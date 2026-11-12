//! Application log: a ring of recent entries, mirrored to `diskmap.log` and sent to the front end.

use crate::config;
use crate::fmt;
use diskmap_core::log::{Level, Ring, format_line, needs_rotation};
use serde::Serialize;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter};

const CAPACITY: usize = 1000;
const FILE_CAP: u64 = 1024 * 1024;
const FILETIME_UNIX_MS: i64 = 11_644_473_600_000;

/// One log entry as sent to the front end.
#[derive(Serialize, Clone)]
pub struct Entry {
    pub id: u64,
    /// Unix time in milliseconds.
    pub time: u64,
    pub level: &'static str,
    pub source: String,
    pub message: String,
}

struct Item {
    time: u64,
    level: Level,
    source: String,
    message: String,
}

/// The log; one per process, see [`get`].
pub struct Log {
    ring: Mutex<Ring<Item>>,
    file: Option<PathBuf>,
    app: OnceLock<AppHandle>,
}

static LOG: OnceLock<Log> = OnceLock::new();

/// The process log.
pub fn get() -> &'static Log {
    LOG.get_or_init(|| Log {
        ring: Mutex::new(Ring::new(CAPACITY)),
        file: config::log_path(),
        app: OnceLock::new(),
    })
}

/// Logs an error.
pub fn error(source: &str, message: impl Into<String>) {
    get().push(Level::Error, source, message.into());
}

/// Logs a warning.
pub fn warn(source: &str, message: impl Into<String>) {
    get().push(Level::Warn, source, message.into());
}

/// Logs a note.
pub fn info(source: &str, message: impl Into<String>) {
    get().push(Level::Info, source, message.into());
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

/// `YYYY-MM-DD HH:MM:SS.mmm` (UTC).
fn stamp(ms: u64) -> String {
    let ft = (ms as i64 + FILETIME_UNIX_MS) * 10_000;
    format!(
        "{}:{:02}.{:03}",
        fmt::date_time(ft),
        ms / 1000 % 60,
        ms % 1000
    )
}

fn entry(id: u64, it: &Item) -> Entry {
    Entry {
        id,
        time: it.time,
        level: it.level.name(),
        source: it.source.clone(),
        message: it.message.clone(),
    }
}

/// Appends a line, moving a full file to `<name>.1` first. Errors are ignored.
fn append(path: &Path, line: &str) {
    let size = std::fs::metadata(path).map_or(0, |m| m.len());
    if needs_rotation(size, line.len() as u64, FILE_CAP) {
        let mut old = path.as_os_str().to_owned();
        old.push(".1");
        let _ = std::fs::remove_file(&old);
        let _ = std::fs::rename(path, &old);
    }
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = f.write_all(line.as_bytes());
    }
}

impl Log {
    fn lock(&self) -> MutexGuard<'_, Ring<Item>> {
        self.ring.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Sends every later entry to the front end as a `log` event.
    pub fn attach(&self, app: AppHandle) {
        let _ = self.app.set(app);
    }

    /// Adds an entry, writes it to the file and tells the front end.
    pub fn push(&self, level: Level, source: &str, message: String) {
        let item = Item {
            time: now_ms(),
            level,
            source: source.to_string(),
            message,
        };
        let mut out = self.app.get().map(|app| (app, entry(0, &item)));
        {
            let mut ring = self.lock();
            if let Some(path) = &self.file {
                append(
                    path,
                    &format_line(&stamp(item.time), level, source, &item.message),
                );
            }
            let id = ring.push(item);
            if let Some((_, e)) = &mut out {
                e.id = id;
            }
        }
        if let Some((app, e)) = out {
            let _ = app.emit("log", e);
        }
    }

    /// Entries held, oldest first.
    pub fn entries(&self) -> Vec<Entry> {
        self.lock().iter().map(|(id, it)| entry(id, it)).collect()
    }

    /// Forgets the entries held; the file is kept.
    pub fn clear(&self) {
        self.lock().clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stamp_is_utc() {
        assert_eq!(stamp(1_655_526_400_123), "2022-06-18 04:26:40.123");
    }

    #[test]
    fn rotates_full_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("diskmap.log");
        std::fs::write(&path, vec![b'x'; FILE_CAP as usize]).unwrap();
        append(&path, "line\n");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "line\n");
        assert!(dir.path().join("diskmap.log.1").exists());
    }
}
