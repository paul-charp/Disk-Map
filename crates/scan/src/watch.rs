//! Live file-system watcher built on `ReadDirectoryChangesW`.

use crate::walk::extended_path;
use crossbeam_channel::{Sender, bounded};
use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::thread::JoinHandle;
use windows::Win32::Foundation::{CloseHandle, ERROR_NOTIFY_ENUM_DIR, HANDLE, WAIT_OBJECT_0};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OVERLAPPED, FILE_LIST_DIRECTORY,
    FILE_NOTIFY_CHANGE_CREATION, FILE_NOTIFY_CHANGE_DIR_NAME, FILE_NOTIFY_CHANGE_FILE_NAME,
    FILE_NOTIFY_CHANGE_LAST_WRITE, FILE_NOTIFY_CHANGE_SIZE, FILE_NOTIFY_INFORMATION,
    FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING, ReadDirectoryChangesW,
};
use windows::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};
use windows::Win32::System::Threading::{CreateEventW, INFINITE, SetEvent, WaitForMultipleObjects};
use windows::core::PCWSTR;

const BUFFER_BYTES: usize = 64 * 1024;

/// What happened to a path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WatchKind {
    Added,
    Removed,
    Modified,
    /// The entry moved from `from` (relative to the root) to the event's path.
    Renamed {
        from: String,
    },
    /// The kernel buffer overflowed (or watching failed): changes were lost and the path is empty.
    /// The consumer should offer a rescan.
    Overflow,
}

/// A raw change notification.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WatchEvent {
    /// Path relative to the watched root, separated by `\`.
    pub path: String,
    pub kind: WatchKind,
}

impl WatchEvent {
    fn new(path: String, kind: WatchKind) -> Self {
        WatchEvent { path, kind }
    }
}

struct SendHandle(HANDLE);

// SAFETY: kernel handles are usable from any thread.
unsafe impl Send for SendHandle {}

/// Watches a directory tree until dropped.
pub struct Watcher {
    stop: HANDLE,
    thread: Option<JoinHandle<()>>,
}

impl Watcher {
    /// Starts watching `root` recursively; returns once the watch is armed.
    /// If the directory cannot be watched, a single [`WatchKind::Overflow`] event is sent instead.
    pub fn start(root: PathBuf, tx: Sender<WatchEvent>) -> Watcher {
        // SAFETY: plain event creation without security attributes or a name.
        let stop = unsafe { CreateEventW(None, true, false, None) }.unwrap_or_default();
        let thread_stop = SendHandle(stop);
        let (ready_tx, ready_rx) = bounded::<()>(1);
        let thread = std::thread::spawn(move || {
            let stop = thread_stop;
            watch_loop(&root, &tx, stop.0, &ready_tx);
            let _ = ready_tx.send(());
        });
        let _ = ready_rx.recv();
        Watcher {
            stop,
            thread: Some(thread),
        }
    }
}

impl Drop for Watcher {
    fn drop(&mut self) {
        // SAFETY: stop is a live event handle owned by this watcher.
        let _ = unsafe { SetEvent(self.stop) };
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
        // SAFETY: the thread is gone; the handle is closed once.
        let _ = unsafe { CloseHandle(self.stop) };
    }
}

fn open_dir(root: &Path) -> windows::core::Result<HANDLE> {
    let mut w = extended_path(root);
    w.push(0);
    // SAFETY: w is NUL-terminated and outlives the call.
    unsafe {
        CreateFileW(
            PCWSTR(w.as_ptr()),
            FILE_LIST_DIRECTORY.0,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            None,
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OVERLAPPED,
            None,
        )
    }
}

fn watch_loop(root: &Path, tx: &Sender<WatchEvent>, stop: HANDLE, ready: &Sender<()>) {
    let overflow = || WatchEvent::new(String::new(), WatchKind::Overflow);
    let dir = match open_dir(root) {
        Ok(h) => h,
        Err(_) => {
            let _ = tx.send(overflow());
            return;
        }
    };
    // SAFETY: plain manual-reset event creation.
    let Ok(event) = (unsafe { CreateEventW(None, true, false, None) }) else {
        let _ = tx.send(overflow());
        // SAFETY: dir was opened above.
        let _ = unsafe { CloseHandle(dir) };
        return;
    };
    let filter = FILE_NOTIFY_CHANGE_FILE_NAME
        | FILE_NOTIFY_CHANGE_DIR_NAME
        | FILE_NOTIFY_CHANGE_SIZE
        | FILE_NOTIFY_CHANGE_LAST_WRITE
        | FILE_NOTIFY_CHANGE_CREATION;
    let mut buf = vec![0u32; BUFFER_BYTES / 4];
    let mut first = true;
    loop {
        let mut ov = OVERLAPPED {
            hEvent: event,
            ..OVERLAPPED::default()
        };
        // SAFETY: buf and ov stay alive and unmoved until the request completes or is cancelled below.
        let armed = unsafe {
            ReadDirectoryChangesW(
                dir,
                buf.as_mut_ptr().cast::<c_void>(),
                BUFFER_BYTES as u32,
                true,
                filter,
                None,
                Some(&mut ov),
                None,
            )
        };
        if first {
            first = false;
            let _ = ready.send(());
        }
        if armed.is_err() {
            let _ = tx.send(overflow());
            break;
        }
        // SAFETY: both handles are live events.
        let signalled = unsafe { WaitForMultipleObjects(&[event, stop], false, INFINITE) };
        let mut bytes = 0u32;
        if signalled != WAIT_OBJECT_0 {
            // SAFETY: cancel the pending request and wait until the kernel is done with buf and ov.
            unsafe {
                let _ = CancelIoEx(dir, Some(&ov));
                let _ = GetOverlappedResult(dir, &ov, &mut bytes, true);
            }
            break;
        }
        // SAFETY: the request completed; ov and bytes are live.
        let result = unsafe { GetOverlappedResult(dir, &ov, &mut bytes, false) };
        let alive = match result {
            Ok(()) if bytes == 0 => tx.send(overflow()).is_ok(),
            Ok(()) => parse_events(&buf, bytes as usize, tx),
            Err(e) if e.code() == ERROR_NOTIFY_ENUM_DIR.to_hresult() => tx.send(overflow()).is_ok(),
            Err(_) => {
                let _ = tx.send(overflow());
                false
            }
        };
        if !alive {
            break;
        }
    }
    // SAFETY: both handles were created above and are closed once.
    unsafe {
        let _ = CloseHandle(event);
        let _ = CloseHandle(dir);
    }
}

/// Sends the events of one completed read; false when the receiver is gone.
fn parse_events(buf: &[u32], len: usize, tx: &Sender<WatchEvent>) -> bool {
    let base = buf.as_ptr().cast::<u8>();
    let mut offset = 0usize;
    let mut old: Option<String> = None;
    loop {
        if offset + 12 > len {
            break;
        }
        // SAFETY: the kernel wrote a chain of DWORD-aligned FILE_NOTIFY_INFORMATION records into buf.
        let info = unsafe { &*base.add(offset).cast::<FILE_NOTIFY_INFORMATION>() };
        let units = info.FileNameLength as usize / 2;
        // SAFETY: FileName holds FileNameLength bytes of UTF-16 inside the record.
        let name = unsafe { std::slice::from_raw_parts(info.FileName.as_ptr(), units) };
        let path = String::from_utf16_lossy(name);
        let event = match info.Action.0 {
            1 => Some(WatchEvent::new(path, WatchKind::Added)),
            2 => Some(WatchEvent::new(path, WatchKind::Removed)),
            3 => Some(WatchEvent::new(path, WatchKind::Modified)),
            4 => {
                if let Some(prev) = old.replace(path)
                    && tx.send(WatchEvent::new(prev, WatchKind::Removed)).is_err()
                {
                    return false;
                }
                None
            }
            5 => Some(match old.take() {
                Some(from) => WatchEvent::new(path, WatchKind::Renamed { from }),
                None => WatchEvent::new(path, WatchKind::Added),
            }),
            _ => None,
        };
        if event.is_some_and(|e| tx.send(e).is_err()) {
            return false;
        }
        if info.NextEntryOffset == 0 {
            break;
        }
        offset += info.NextEntryOffset as usize;
    }
    match old {
        Some(prev) => tx.send(WatchEvent::new(prev, WatchKind::Removed)).is_ok(),
        None => true,
    }
}
