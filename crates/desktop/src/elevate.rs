//! Starting programs with administrator rights.

use diskmap_core::schedule::command_line;
use std::ffi::OsStr;
use std::mem::size_of;
use std::os::windows::ffi::OsStrExt;
use windows::Win32::Foundation::CloseHandle;
use windows::Win32::System::Threading::{GetExitCodeProcess, INFINITE, WaitForSingleObject};
use windows::Win32::UI::Shell::{
    SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW,
};
use windows::Win32::UI::WindowsAndMessaging::{SW_HIDE, SW_SHOWNORMAL};
use windows::core::PCWSTR;

fn wide(s: &OsStr) -> Vec<u16> {
    s.encode_wide().chain(std::iter::once(0)).collect()
}

/// Starts this executable again as administrator with `paths` as arguments, as its own instance.
/// False when the user declined the prompt or the launch failed; this process keeps running either way.
pub fn relaunch(paths: &[String]) -> bool {
    let Ok(exe) = std::env::current_exe() else {
        return false;
    };
    let file = wide(exe.as_os_str());
    let args: Vec<String> = paths
        .iter()
        .cloned()
        .chain(std::iter::once("--new-instance".to_string()))
        .collect();
    let params = wide(OsStr::new(&command_line(&args)));
    let verb = wide(OsStr::new("runas"));
    let mut info = SHELLEXECUTEINFOW {
        cbSize: size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOASYNC,
        lpVerb: PCWSTR(verb.as_ptr()),
        lpFile: PCWSTR(file.as_ptr()),
        lpParameters: PCWSTR(params.as_ptr()),
        nShow: SW_SHOWNORMAL.0,
        ..SHELLEXECUTEINFOW::default()
    };
    // SAFETY: info points to NUL-terminated strings that outlive the call.
    unsafe { ShellExecuteExW(&mut info) }.is_ok()
}

/// Runs `program` as administrator with a hidden window and waits for its exit code.
/// Fails when the user declined the prompt.
pub fn run_elevated(program: &str, args: &[String]) -> Result<u32, String> {
    let file = wide(OsStr::new(program));
    let params = wide(OsStr::new(&command_line(args)));
    let verb = wide(OsStr::new("runas"));
    let mut info = SHELLEXECUTEINFOW {
        cbSize: size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOASYNC | SEE_MASK_NOCLOSEPROCESS,
        lpVerb: PCWSTR(verb.as_ptr()),
        lpFile: PCWSTR(file.as_ptr()),
        lpParameters: PCWSTR(params.as_ptr()),
        nShow: SW_HIDE.0,
        ..SHELLEXECUTEINFOW::default()
    };
    // SAFETY: info points to NUL-terminated strings that outlive the call.
    unsafe { ShellExecuteExW(&mut info) }.map_err(|e| format!("Elevation failed: {e}"))?;
    if info.hProcess.is_invalid() {
        return Err("Elevation failed: no process".into());
    }
    let mut code = 1;
    // SAFETY: hProcess is a valid process handle owned here and closed once.
    unsafe {
        WaitForSingleObject(info.hProcess, INFINITE);
        let got = GetExitCodeProcess(info.hProcess, &mut code);
        let _ = CloseHandle(info.hProcess);
        got.map_err(|e| e.to_string())?;
    }
    Ok(code)
}
