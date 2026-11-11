//! Parallel directory walk using `GetFileInformationByHandleEx`.

use crate::event::{DirBatch, DirToken, Entry, ScanEvent, Scanner};
use crossbeam_channel::Sender;
use rayon::{Scope, ThreadPoolBuilder};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use windows::Win32::Foundation::{CloseHandle, ERROR_NO_MORE_FILES, HANDLE};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT,
    FILE_FLAG_BACKUP_SEMANTICS, FILE_ID_BOTH_DIR_INFO, FILE_LIST_DIRECTORY, FILE_SHARE_DELETE,
    FILE_SHARE_READ, FILE_SHARE_WRITE, FileIdBothDirectoryInfo, FileIdBothDirectoryRestartInfo,
    GetFileInformationByHandleEx, OPEN_EXISTING,
};
use windows::core::PCWSTR;

const BUFFER_BYTES: usize = 64 * 1024;
const MAX_BATCH_ENTRIES: usize = 4096;

thread_local! {
    static BUFFER: RefCell<Vec<u64>> = RefCell::new(vec![0; BUFFER_BYTES / 8]);
}

/// Parallel walker: one rayon task per directory.
pub struct WalkScanner {
    threads: usize,
}

impl WalkScanner {
    /// Creates a walker with `threads` workers, or the default when `None`.
    pub fn new(threads: Option<usize>) -> Self {
        WalkScanner {
            threads: threads.filter(|&n| n > 0).unwrap_or_else(default_threads),
        }
    }
}

impl Default for WalkScanner {
    fn default() -> Self {
        Self::new(None)
    }
}

/// Available parallelism clamped to 2..=16.
pub fn default_threads() -> usize {
    std::thread::available_parallelism()
        .map_or(4, |n| n.get())
        .clamp(2, 16)
}

/// Absolute form of `path` with `\` separators and no `\\?\` prefix.
pub fn absolute_root(path: &Path) -> String {
    let abs = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    let s = abs.to_string_lossy().replace('/', "\\");
    if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{rest}")
    } else if let Some(rest) = s.strip_prefix(r"\\?\") {
        rest.to_string()
    } else {
        s
    }
}

pub(crate) fn extended_path(path: &Path) -> Vec<u16> {
    let s = absolute_root(path);
    let ext = match s.strip_prefix(r"\\") {
        Some(unc) => format!(r"\\?\UNC\{unc}"),
        None => format!(r"\\?\{s}"),
    };
    ext.encode_utf16().collect()
}

fn display_path(wide: &[u16]) -> String {
    let s = String::from_utf16_lossy(wide);
    if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{rest}")
    } else if let Some(rest) = s.strip_prefix(r"\\?\") {
        rest.to_string()
    } else {
        s
    }
}

fn win32_code(e: &windows::core::Error) -> u32 {
    let hr = e.code().0 as u32;
    if hr >> 16 == 0x8007 { hr & 0xFFFF } else { hr }
}

/// Closes the handle on drop.
struct DirHandle(HANDLE);

impl DirHandle {
    fn open(path: &[u16]) -> windows::core::Result<Self> {
        let mut z = Vec::with_capacity(path.len() + 1);
        z.extend_from_slice(path);
        z.push(0);
        // SAFETY: z is NUL-terminated and outlives the call.
        let h = unsafe {
            CreateFileW(
                PCWSTR(z.as_ptr()),
                FILE_LIST_DIRECTORY.0,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                None,
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS,
                None,
            )
        }?;
        Ok(DirHandle(h))
    }
}

impl Drop for DirHandle {
    fn drop(&mut self) {
        // SAFETY: the handle was returned by CreateFileW and is closed once.
        let _ = unsafe { CloseHandle(self.0) };
    }
}

struct Ctx {
    tx: Sender<ScanEvent>,
    cancel: Arc<AtomicBool>,
    next_token: AtomicU32,
}

impl Ctx {
    fn send(&self, event: ScanEvent) {
        if self.tx.send(event).is_err() {
            self.cancel.store(true, Ordering::Relaxed);
        }
    }
}

struct BatchBuilder {
    token: DirToken,
    names: String,
    entries: Vec<Entry>,
    children: Vec<(DirToken, Vec<u16>)>,
}

impl BatchBuilder {
    fn new(token: DirToken) -> Self {
        BatchBuilder {
            token,
            names: String::new(),
            entries: Vec::new(),
            children: Vec::new(),
        }
    }

    /// Sends the batch, then spawns the tasks of the sub-directories it announced.
    fn flush<'s>(&mut self, scope: &Scope<'s>, ctx: &'s Ctx) {
        if self.entries.is_empty() {
            return;
        }
        ctx.send(ScanEvent::Batch(DirBatch {
            parent: self.token,
            names: std::mem::take(&mut self.names),
            entries: std::mem::take(&mut self.entries),
        }));
        for (token, path) in self.children.drain(..) {
            scope.spawn(move |s| scan_dir(s, ctx, token, path));
        }
    }
}

fn scan_dir<'s>(scope: &Scope<'s>, ctx: &'s Ctx, token: DirToken, path: Vec<u16>) {
    if ctx.cancel.load(Ordering::Relaxed) {
        return;
    }
    let handle = match DirHandle::open(&path) {
        Ok(h) => h,
        Err(e) => {
            ctx.send(ScanEvent::Error {
                path: display_path(&path),
                code: win32_code(&e),
            });
            return;
        }
    };
    let mut batch = BatchBuilder::new(token);
    let mut restart = true;
    let mut utf8 = String::new();
    loop {
        let result = BUFFER.with_borrow_mut(|buf| {
            let class = if restart {
                FileIdBothDirectoryRestartInfo
            } else {
                FileIdBothDirectoryInfo
            };
            // SAFETY: buf is a live 8-byte-aligned buffer of BUFFER_BYTES bytes.
            let r = unsafe {
                GetFileInformationByHandleEx(
                    handle.0,
                    class,
                    buf.as_mut_ptr().cast(),
                    BUFFER_BYTES as u32,
                )
            };
            if r.is_ok() {
                parse_buffer(buf, ctx, &path, &mut batch, &mut utf8, scope);
            }
            r
        });
        restart = false;
        match result {
            Ok(()) => {}
            Err(e) => {
                let code = win32_code(&e);
                if code != ERROR_NO_MORE_FILES.0 {
                    ctx.send(ScanEvent::Error {
                        path: display_path(&path),
                        code,
                    });
                }
                break;
            }
        }
        if ctx.cancel.load(Ordering::Relaxed) {
            break;
        }
    }
    batch.flush(scope, ctx);
}

fn parse_buffer<'s>(
    buf: &[u64],
    ctx: &'s Ctx,
    dir_path: &[u16],
    batch: &mut BatchBuilder,
    utf8: &mut String,
    scope: &Scope<'s>,
) {
    let base = buf.as_ptr().cast::<u8>();
    let mut offset = 0usize;
    loop {
        // SAFETY: the kernel wrote a chain of aligned FILE_ID_BOTH_DIR_INFO records into buf.
        let info = unsafe { &*base.add(offset).cast::<FILE_ID_BOTH_DIR_INFO>() };
        let name_len = info.FileNameLength as usize / 2;
        // SAFETY: FileName holds FileNameLength bytes of UTF-16 inside the record.
        let name = unsafe { std::slice::from_raw_parts(info.FileName.as_ptr(), name_len) };
        let is_dot = matches!(name, [0x2E] | [0x2E, 0x2E]);
        if !is_dot {
            push_entry(info, name, ctx, dir_path, batch, utf8);
            if batch.entries.len() >= MAX_BATCH_ENTRIES {
                batch.flush(scope, ctx);
            }
        }
        if info.NextEntryOffset == 0 {
            break;
        }
        offset += info.NextEntryOffset as usize;
    }
}

fn push_entry(
    info: &FILE_ID_BOTH_DIR_INFO,
    name: &[u16],
    ctx: &Ctx,
    dir_path: &[u16],
    batch: &mut BatchBuilder,
    utf8: &mut String,
) {
    utf8.clear();
    utf8.extend(char::decode_utf16(name.iter().copied()).map(|r| r.unwrap_or('\u{FFFD}')));
    let start = batch.names.len() as u32;
    batch.names.push_str(utf8);
    let end = batch.names.len() as u32;

    let attrs = info.FileAttributes;
    let reparse = attrs & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0;
    let is_dir = attrs & FILE_ATTRIBUTE_DIRECTORY.0 != 0;
    let (logical, allocated) = if reparse {
        (0, 0)
    } else {
        (
            info.EndOfFile.max(0) as u64,
            info.AllocationSize.max(0) as u64,
        )
    };

    let dir = (is_dir && !reparse).then(|| {
        let token = DirToken(ctx.next_token.fetch_add(1, Ordering::Relaxed));
        let mut child = Vec::with_capacity(dir_path.len() + 1 + name.len());
        child.extend_from_slice(dir_path);
        if child.last() != Some(&(b'\\' as u16)) {
            child.push(b'\\' as u16);
        }
        child.extend_from_slice(name);
        batch.children.push((token, child));
        token
    });

    batch.entries.push(Entry {
        name: start..end,
        logical,
        allocated,
        ctime: info.CreationTime,
        mtime: info.LastWriteTime,
        atime: info.LastAccessTime,
        attrs,
        dir,
    });
}

impl Scanner for WalkScanner {
    fn start(self: Box<Self>, root: PathBuf, tx: Sender<ScanEvent>, cancel: Arc<AtomicBool>) {
        let threads = self.threads;
        std::thread::spawn(move || {
            let ctx = Ctx {
                tx,
                cancel,
                next_token: AtomicU32::new(1),
            };
            match ThreadPoolBuilder::new().num_threads(threads).build() {
                Ok(pool) => {
                    let path = extended_path(&root);
                    pool.scope(|s| scan_dir(s, &ctx, DirToken::ROOT, path));
                }
                Err(_) => ctx.send(ScanEvent::Error {
                    path: absolute_root(&root),
                    code: 0,
                }),
            }
            let cancelled = ctx.cancel.load(Ordering::Relaxed);
            ctx.send(ScanEvent::Done { cancelled });
        });
    }
}
