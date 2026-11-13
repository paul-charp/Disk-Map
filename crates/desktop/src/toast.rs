//! Windows toast notification after a headless snapshot run.

use crate::log;
use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use windows::Data::Xml::Dom::XmlDocument;
use windows::UI::Notifications::{ToastNotification, ToastNotificationManager};
use windows::Win32::System::Registry::{HKEY_CURRENT_USER, REG_SZ, RegSetKeyValueW};
use windows::core::{HSTRING, PCWSTR};

/// AppUserModelID of the app, the Tauri identifier.
pub const AUMID: &str = "dev.diskmap.app";
const DISPLAY_NAME: &str = "diskmap";

fn wide(s: &str) -> Vec<u16> {
    OsStr::new(s)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Toast content: a title line and a body line.
fn toast_xml(title: &str, body: &str) -> String {
    format!(
        r#"<toast><visual><binding template="ToastGeneric"><text>{}</text><text>{}</text></binding></visual></toast>"#,
        xml_escape(title),
        xml_escape(body)
    )
}

/// Registers the AppUserModelID under `HKCU\Software\Classes\AppUserModelId` so an
/// unpackaged app without a Start menu shortcut can show toasts.
fn register() {
    let key = wide(&format!(r"Software\Classes\AppUserModelId\{AUMID}"));
    let name = wide("DisplayName");
    let value = wide(DISPLAY_NAME);
    // SAFETY: all pointers are NUL-terminated buffers alive for the call; the size is in bytes.
    let err = unsafe {
        RegSetKeyValueW(
            HKEY_CURRENT_USER,
            PCWSTR(key.as_ptr()),
            PCWSTR(name.as_ptr()),
            REG_SZ.0,
            Some(value.as_ptr().cast()),
            (value.len() * 2) as u32,
        )
    };
    if err.is_err() {
        log::warn("toast", format!("Cannot register {AUMID}: {err:?}"));
    }
}

fn try_show(title: &str, body: &str) -> windows::core::Result<()> {
    register();
    let doc = XmlDocument::new()?;
    doc.LoadXml(&HSTRING::from(toast_xml(title, body)))?;
    let toast = ToastNotification::CreateToastNotification(&doc)?;
    ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(AUMID))?.Show(&toast)
}

/// Shows a toast; a failure is only logged.
pub fn show(title: &str, body: &str) {
    if let Err(e) = try_show(title, body) {
        log::warn("toast", format!("No toast: {e}"));
    }
}
