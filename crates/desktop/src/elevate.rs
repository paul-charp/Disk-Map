//! Relaunching the app with administrator rights.

use std::ffi::OsStr;
use std::mem::size_of;
use std::os::windows::ffi::OsStrExt;
use windows::Win32::UI::Shell::{SEE_MASK_NOASYNC, SHELLEXECUTEINFOW, ShellExecuteExW};
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
use windows::core::PCWSTR;

fn wide(s: &OsStr) -> Vec<u16> {
    s.encode_wide().chain(std::iter::once(0)).collect()
}

/// Quotes one argument for a Windows command line.
fn quote(arg: &str) -> String {
    let mut out = String::from('"');
    let mut slashes = 0;
    for c in arg.chars() {
        match c {
            '\\' => slashes += 1,
            '"' => {
                out.push_str(&"\\".repeat(slashes * 2 + 1));
                out.push('"');
                slashes = 0;
            }
            _ => {
                out.push_str(&"\\".repeat(slashes));
                out.push(c);
                slashes = 0;
            }
        }
    }
    out.push_str(&"\\".repeat(slashes * 2));
    out.push('"');
    out
}

fn command_line(paths: &[String]) -> String {
    paths.iter().map(|p| quote(p)).collect::<Vec<_>>().join(" ")
}

/// Starts this executable again as administrator with `paths` as arguments.
/// False when the user declined the prompt or the launch failed; this process keeps running either way.
pub fn relaunch(paths: &[String]) -> bool {
    let Ok(exe) = std::env::current_exe() else {
        return false;
    };
    let file = wide(exe.as_os_str());
    let params = wide(OsStr::new(&command_line(paths)));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quotes_paths() {
        assert_eq!(quote(r"C:\"), r#""C:\\""#);
        assert_eq!(quote(r"D:\my files\x"), r#""D:\my files\x""#);
        assert_eq!(
            command_line(&[r"C:\a".into(), r"E:\".into()]),
            r#""C:\a" "E:\\""#
        );
    }
}
