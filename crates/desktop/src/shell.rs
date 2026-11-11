//! Explorer integration.

use std::os::windows::process::CommandExt;
use std::process::Command;

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

/// Stub for the native Explorer shell context menu (`IContextMenu`).
#[allow(dead_code)]
pub fn show_explorer_shell_menu(_path: &str) {}
