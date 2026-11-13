//! The scheduled snapshot task in Task Scheduler, managed with `schtasks.exe`.

use crate::config::Schedule;
use crate::{elevate, paths};
use diskmap_core::schedule::{create_args, delete_args, query_args, task_xml};
use diskmap_scan::is_elevated;
use std::os::windows::process::CommandExt;
use std::process::Command;

const SCHTASKS: &str = "schtasks.exe";
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const XML_FILE: &str = "schedule.xml";

/// Runs `schtasks`, as administrator when `elevated` and this process is not.
fn schtasks(args: &[String], elevated: bool) -> Result<(), String> {
    if elevated && !is_elevated() {
        return match elevate::run_elevated(SCHTASKS, args)? {
            0 => Ok(()),
            code => Err(format!("schtasks failed ({code})")),
        };
    }
    let out = Command::new(SCHTASKS)
        .args(args)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| format!("schtasks: {e}"))?;
    if out.status.success() {
        return Ok(());
    }
    let text = String::from_utf8_lossy(&out.stderr).trim().to_string();
    Err(if text.is_empty() {
        format!("schtasks failed ({})", out.status)
    } else {
        text
    })
}

/// Whether the task exists.
pub fn exists() -> bool {
    schtasks(&query_args(), false).is_ok()
}

/// UTF-16 LE with a byte order mark, the encoding `schtasks /XML` reads reliably.
fn utf16_bom(text: &str) -> Vec<u8> {
    let mut out = vec![0xFF, 0xFE];
    out.extend(text.encode_utf16().flat_map(u16::to_le_bytes));
    out
}

/// Runs `schtasks`, retrying as administrator when the task being replaced was elevated.
fn schtasks_over(args: &[String], elevated: bool, old_elevated: bool) -> Result<(), String> {
    schtasks(args, elevated).or_else(|e| {
        if old_elevated && !elevated {
            schtasks(args, true)
        } else {
            Err(e)
        }
    })
}

fn create(schedule: &Schedule, old_elevated: bool) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let plan = schedule.plan(&exe.to_string_lossy())?;
    let xml = paths::ensure_data_dir()
        .map_err(|e| e.to_string())?
        .join(XML_FILE);
    std::fs::write(&xml, utf16_bom(&task_xml(&plan))).map_err(|e| e.to_string())?;
    let args = create_args(&xml.to_string_lossy());
    let result = schtasks_over(&args, plan.elevated, old_elevated);
    let _ = std::fs::remove_file(&xml);
    result
}

/// Creates, replaces or deletes the task to match `new`. `old` tells whether an existing task
/// was elevated, which deleting it may need.
pub fn apply(old: &Schedule, new: &Schedule) -> Result<(), String> {
    let was_elevated = old.enabled && old.elevated;
    if new.enabled {
        create(new, was_elevated)
    } else if exists() {
        schtasks_over(&delete_args(), false, was_elevated)
    } else {
        Ok(())
    }
}
