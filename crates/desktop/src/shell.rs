//! Explorer integration.

use std::os::windows::process::CommandExt;
use std::process::Command;
use windows::Win32::System::Com::{
    COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoInitializeEx, CoUninitialize,
};
use windows::Win32::UI::Shell::{
    FO_DELETE, FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_NOERRORUI, FOF_SILENT, FOF_WANTNUKEWARNING,
    SHFILEOPSTRUCTW, SHFileOperationW, SHOP_FILEPATH, SHObjectProperties,
};
use windows::core::PCWSTR;

const CREATE_NEW_CONSOLE: u32 = 0x10;

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Opens a file or folder with its default handler.
pub fn open(path: &str) {
    let _ = Command::new("explorer.exe").arg(path).spawn();
}

/// Shows the item selected in an Explorer window.
pub fn reveal(path: &str) {
    let _ = Command::new("explorer.exe")
        .raw_arg(format!("/select,\"{path}\""))
        .spawn();
}

/// Opens a terminal in a folder: Windows Terminal when installed, else a new `cmd.exe` console.
pub fn terminal(dir: &str) -> Result<(), String> {
    let wt = Command::new("wt.exe")
        .args(["-d", "."])
        .current_dir(dir)
        .spawn();
    if wt.is_ok() {
        return Ok(());
    }
    Command::new("cmd.exe")
        .arg("/K")
        .current_dir(dir)
        .creation_flags(CREATE_NEW_CONSOLE)
        .spawn()
        .map(drop)
        .map_err(|e| e.to_string())
}

/// Shows the Explorer Properties dialog of a file or folder. Call it on a thread with COM.
pub fn properties(path: &str) -> Result<(), String> {
    let name = wide(path);
    // SAFETY: name is NUL-terminated and outlives the call; no property page is named.
    let ok =
        unsafe { SHObjectProperties(None, SHOP_FILEPATH, PCWSTR(name.as_ptr()), PCWSTR::null()) };
    ok.ok().map_err(|e| e.message())
}

/// Keeps COM initialised on the current thread while alive.
struct Com(bool);

impl Com {
    fn init() -> Self {
        // SAFETY: plain per-thread COM initialisation, undone in Drop when it succeeded.
        let hr = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) };
        Com(hr.is_ok())
    }
}

impl Drop for Com {
    fn drop(&mut self) {
        if self.0 {
            // SAFETY: pairs with the successful CoInitializeEx of this thread.
            unsafe { CoUninitialize() };
        }
    }
}

/// Moves a file or folder to the Recycle Bin; blocks until done.
pub fn recycle(path: &str) -> Result<(), String> {
    let _com = Com::init();
    // pFrom is a list ending with an empty string, so it needs two NULs.
    let mut from = wide(path.trim_end_matches('\\'));
    from.push(0);
    let flags =
        FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_NOERRORUI | FOF_SILENT | FOF_WANTNUKEWARNING;
    let mut op = SHFILEOPSTRUCTW {
        wFunc: FO_DELETE,
        pFrom: PCWSTR(from.as_ptr()),
        fFlags: flags.0 as u16,
        ..SHFILEOPSTRUCTW::default()
    };
    // SAFETY: op points to a double-NUL-terminated path list that outlives the call.
    let code = unsafe { SHFileOperationW(&mut op) };
    if code != 0 {
        return Err(format!("Recycle failed ({code:#x})"));
    }
    if op.fAnyOperationsAborted.as_bool() {
        return Err("Recycle cancelled".into());
    }
    Ok(())
}

/// Stub for the native Explorer shell context menu (`IContextMenu`).
#[allow(dead_code)]
pub fn show_explorer_shell_menu(_path: &str) {}
