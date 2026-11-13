//! Settings persisted in `diskmap.toml` at the location chosen by `paths`.

use crate::{log, paths};
use diskmap_core::classes::{FileClass, FileClasses};
use diskmap_core::schedule::{Plan, day_index, parse_time};
use serde::{Deserialize, Serialize};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

const MAX_RECENT: usize = 8;
const SCALE_RANGE: (u32, u32) = (80, 200);
const MIN_TILE_RANGE: (u32, u32) = (2, 16);
const MAX_THREADS: u32 = 64;
const TEXT_SIZES: [&str; 3] = ["small", "medium", "large"];
const MAX_KEEP: u32 = 100;
const BACKENDS: [&str; 3] = ["auto", "mft", "walk"];
const EVERY: [&str; 2] = ["daily", "weekly"];

/// Which scan backend a scan should use.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackendPref {
    /// MFT when it is available, the directory walk otherwise.
    Auto,
    /// MFT, falling back to the walk when it is unavailable.
    Mft,
    /// Always the directory walk.
    Walk,
}

/// How file tiles are coloured.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FileColors {
    /// The theme's file colour.
    #[default]
    Accent,
    /// The colour of the file class.
    Class,
    /// A ramp by modification time.
    Age,
}

impl FileColors {
    /// Parses `accent`, `class` or `age`.
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "accent" => Some(FileColors::Accent),
            "class" => Some(FileColors::Class),
            "age" => Some(FileColors::Age),
            _ => None,
        }
    }
}

/// Reads a colour mode, or the former `class_colors` flag.
fn file_colors_compat<'de, D: serde::Deserializer<'de>>(d: D) -> Result<FileColors, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Value {
        Flag(bool),
        Mode(FileColors),
    }
    Ok(match Value::deserialize(d)? {
        Value::Flag(true) => FileColors::Class,
        Value::Flag(false) => FileColors::Accent,
        Value::Mode(m) => m,
    })
}

/// A previously scanned path with the size found.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Recent {
    pub path: String,
    pub size: u64,
}

/// A file class as stored in the configuration and edited in the settings.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClassDef {
    pub name: String,
    /// `#rrggbb`.
    pub color: String,
    pub extensions: Vec<String>,
}

/// Periodic snapshot task, created in Task Scheduler by `schedule_set`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Schedule {
    pub enabled: bool,
    /// `daily` or `weekly`.
    pub every: String,
    /// Day of a weekly run, `mon` to `sun`.
    pub day: String,
    /// Local start time, `HH:MM`.
    pub time: String,
    /// Folders to snapshot, normalised, without duplicates.
    pub roots: Vec<String>,
    /// Run with highest privileges so the MFT backend can be used.
    pub elevated: bool,
}

impl Default for Schedule {
    fn default() -> Self {
        Schedule {
            enabled: false,
            every: "daily".into(),
            day: "mon".into(),
            time: "12:00".into(),
            roots: Vec::new(),
            elevated: false,
        }
    }
}

impl Schedule {
    /// Brings every value back into its valid range.
    pub fn sanitize(&mut self) {
        let def = Schedule::default();
        if !EVERY.contains(&self.every.as_str()) {
            self.every = def.every;
        }
        self.day = match day_index(&self.day) {
            Some(i) => diskmap_core::schedule::DAYS[i].into(),
            None => def.day,
        };
        if parse_time(&self.time).is_none() {
            self.time = def.time;
        }
        let mut roots: Vec<String> = Vec::new();
        for r in self.roots.iter().map(|r| normalize(r)) {
            if !r.is_empty() && !roots.iter().any(|k| same_path(k, &r)) {
                roots.push(r);
            }
        }
        self.roots = roots;
    }

    /// What the task runs and when, for the app at `exe`.
    pub fn plan(&self, exe: &str) -> Result<Plan, String> {
        let (hour, minute) = parse_time(&self.time).ok_or("Invalid time")?;
        if self.roots.is_empty() {
            return Err("No folders to snapshot".into());
        }
        Ok(Plan {
            exe: exe.into(),
            roots: self.roots.clone(),
            weekday: (self.every == "weekly").then(|| day_index(&self.day).unwrap_or(0)),
            hour,
            minute,
            elevated: self.elevated,
        })
    }
}

/// Persisted settings.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub theme: String,
    pub recent: Vec<Recent>,
    /// Pinned folders, normalised, without duplicates.
    pub pins: Vec<String>,
    /// Defaults for new tabs.
    pub detail: u32,
    #[serde(alias = "class_colors", deserialize_with = "file_colors_compat")]
    pub file_colors: FileColors,
    pub show_free: bool,
    pub show_unknown: bool,
    pub show_side: bool,
    /// Webview zoom in percent.
    pub ui_scale: u32,
    /// `small`, `medium` or `large`.
    pub map_text: String,
    pub animation: bool,
    /// Minimum tile size in CSS pixels.
    pub min_tile: u32,
    /// Scan worker threads, 0 for automatic.
    pub threads: u32,
    /// `auto`, `mft` or `walk`.
    pub backend: String,
    pub watch: bool,
    pub highlight_changes: bool,
    /// Check for a new release at startup.
    pub check_updates: bool,
    /// Save a snapshot after every full scan.
    pub auto_snapshot: bool,
    /// Automatic snapshots kept per scan root; manual ones are never pruned.
    pub keep_snapshots: u32,
    pub classes: Vec<ClassDef>,
    pub schedule: Schedule,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            theme: "graphite".into(),
            recent: Vec::new(),
            pins: Vec::new(),
            detail: 3,
            file_colors: FileColors::Accent,
            show_free: true,
            show_unknown: true,
            show_side: false,
            ui_scale: 100,
            map_text: "medium".into(),
            animation: true,
            min_tile: 3,
            threads: 0,
            backend: "auto".into(),
            watch: false,
            highlight_changes: false,
            check_updates: true,
            auto_snapshot: true,
            keep_snapshots: 5,
            classes: class_defs(&FileClasses::default()),
            schedule: Schedule::default(),
        }
    }
}

/// The classes of a lookup as editable definitions.
pub fn class_defs(classes: &FileClasses) -> Vec<ClassDef> {
    classes
        .classes()
        .iter()
        .map(|c| ClassDef {
            name: c.name.clone(),
            color: hex(c.color),
            extensions: c.extensions.clone(),
        })
        .collect()
}

/// `#rrggbb` of a colour.
pub fn hex(c: [u8; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
}

fn parse_hex(s: &str) -> [u8; 3] {
    let v = u32::from_str_radix(s.trim_start_matches('#'), 16).unwrap_or(0x495057);
    [(v >> 16) as u8, (v >> 8) as u8, v as u8]
}

/// Path of the log file, in the data folder.
pub fn log_path() -> Option<PathBuf> {
    Some(paths::ensure_data_dir().ok()?.join("diskmap.log"))
}

fn read(path: &Path) -> Option<Config> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == ErrorKind::NotFound => return None,
        Err(e) => {
            log::warn("config", format!("Cannot read {}: {e}", path.display()));
            return None;
        }
    };
    toml::from_str(&text)
        .map_err(|e| log::error("config", format!("Invalid {}: {e}", path.display())))
        .ok()
}

impl Config {
    /// Loads the file, falling back to defaults.
    pub fn load() -> Self {
        let mut cfg = paths::config_path()
            .and_then(|p| read(&p))
            .unwrap_or_default();
        cfg.sanitize();
        cfg
    }

    /// Writes the file; failures are logged.
    pub fn save(&self) {
        let Some(path) = paths::config_path() else {
            return;
        };
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            let _ = std::fs::create_dir_all(dir);
        }
        let result = toml::to_string_pretty(self)
            .map_err(|e| e.to_string())
            .and_then(|text| std::fs::write(&path, text).map_err(|e| e.to_string()));
        if let Err(e) = result {
            log::error("config", format!("Cannot save {}: {e}", path.display()));
        }
    }

    /// Brings every value back into its valid range.
    pub fn sanitize(&mut self) {
        self.detail = self.detail.clamp(1, 12);
        self.ui_scale = self.ui_scale.clamp(SCALE_RANGE.0, SCALE_RANGE.1);
        self.min_tile = self.min_tile.clamp(MIN_TILE_RANGE.0, MIN_TILE_RANGE.1);
        self.threads = self.threads.min(MAX_THREADS);
        self.keep_snapshots = self.keep_snapshots.clamp(1, MAX_KEEP);
        if !TEXT_SIZES.contains(&self.map_text.as_str()) {
            self.map_text = "medium".into();
        }
        if !BACKENDS.contains(&self.backend.as_str()) {
            self.backend = "auto".into();
        }
        for p in std::mem::take(&mut self.pins) {
            self.set_pin(&p, true);
        }
        self.schedule.sanitize();
        for c in &mut self.classes {
            c.name = c.name.trim().to_string();
            if c.name.is_empty() {
                c.name = "Class".into();
            }
        }
    }

    /// Replaces every setting but the recent list, the pins and the schedule with those of `new`.
    pub fn replace(&mut self, mut new: Config) {
        new.recent = std::mem::take(&mut self.recent);
        new.pins = std::mem::take(&mut self.pins);
        new.schedule = std::mem::take(&mut self.schedule);
        new.sanitize();
        *self = new;
    }

    /// File class lookup built from the configured definitions.
    pub fn file_classes(&self) -> FileClasses {
        FileClasses::new(
            self.classes
                .iter()
                .map(|c| FileClass {
                    name: c.name.clone(),
                    color: parse_hex(&c.color),
                    extensions: c.extensions.clone(),
                })
                .collect(),
        )
    }

    /// Scale of label sizes on the map.
    pub fn text_scale(&self) -> f32 {
        match self.map_text.as_str() {
            "small" => 0.9,
            "large" => 1.2,
            _ => 1.0,
        }
    }

    /// Worker threads for a scan, `None` for automatic.
    pub fn scan_threads(&self) -> Option<usize> {
        (self.threads > 0).then_some(self.threads as usize)
    }

    /// The configured scan backend.
    pub fn backend_pref(&self) -> BackendPref {
        match self.backend.as_str() {
            "mft" => BackendPref::Mft,
            "walk" => BackendPref::Walk,
            _ => BackendPref::Auto,
        }
    }

    /// Moves `path` to the top of the recent list.
    pub fn push_recent(&mut self, path: &str) {
        let size = self
            .recent
            .iter()
            .find(|r| r.path == path)
            .map_or(0, |r| r.size);
        self.recent.retain(|r| r.path != path);
        self.recent.insert(
            0,
            Recent {
                path: path.into(),
                size,
            },
        );
        self.recent.truncate(MAX_RECENT);
    }

    /// Records the size of a finished scan.
    pub fn set_recent_size(&mut self, path: &str, size: u64) {
        if let Some(r) = self.recent.iter_mut().find(|r| r.path == path) {
            r.size = size;
        }
    }

    /// Whether `path` is pinned.
    pub fn is_pinned(&self, path: &str) -> bool {
        let path = normalize(path);
        self.pins.iter().any(|p| same_path(p, &path))
    }

    /// Pins or unpins `path`; returns whether the list changed.
    pub fn set_pin(&mut self, path: &str, on: bool) -> bool {
        let path = normalize(path);
        let before = self.pins.len();
        if !on {
            self.pins.retain(|p| !same_path(p, &path));
        } else if !path.is_empty() && !self.is_pinned(&path) {
            self.pins.push(path);
        }
        self.pins.len() != before
    }

    /// Size of the last scan of `path`, 0 when unknown.
    pub fn known_size(&self, path: &str) -> u64 {
        let path = normalize(path);
        self.recent
            .iter()
            .find(|r| same_path(&normalize(&r.path), &path))
            .map_or(0, |r| r.size)
    }
}

/// Trims spaces and trailing separators, keeping roots such as `C:\` and `/`.
pub fn normalize(path: &str) -> String {
    let path = path.trim();
    let trimmed = path.trim_end_matches(['\\', '/']);
    if trimmed.is_empty() {
        return path.chars().take(1).collect();
    }
    if trimmed.ends_with(':') && trimmed.len() < path.len() {
        return format!("{trimmed}\\");
    }
    trimmed.to_string()
}

/// Path equality, case-insensitive on Windows.
pub fn same_path(a: &str, b: &str) -> bool {
    if cfg!(windows) {
        a.to_lowercase() == b.to_lowercase()
    } else {
        a == b
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replace_keeps_recent_and_clamps() {
        let mut cfg = Config::default();
        cfg.push_recent("C:\\a");
        let new = Config {
            ui_scale: 500,
            min_tile: 0,
            map_text: "huge".into(),
            ..Config::default()
        };
        cfg.replace(new);
        assert_eq!(cfg.recent.len(), 1);
        assert_eq!((cfg.ui_scale, cfg.min_tile), (200, 2));
        assert_eq!(cfg.map_text, "medium");
    }

    #[test]
    fn pins_are_normalised_and_unique() {
        let mut cfg = Config::default();
        assert!(cfg.set_pin("C:\\data\\ ", true));
        assert!(!cfg.set_pin("C:\\data", true));
        assert!(cfg.set_pin("D:\\", true));
        assert_eq!(cfg.pins, ["C:\\data", "D:\\"]);
        assert!(cfg.is_pinned("C:\\data\\"));
        assert!(cfg.set_pin("C:\\data", false));
        assert!(!cfg.set_pin("C:\\data", false));
        assert_eq!(cfg.pins, ["D:\\"]);
        assert_eq!(normalize("D:"), "D:");
        assert_eq!(normalize("/"), "/");
    }

    #[test]
    fn sanitize_dedups_pins_and_replace_keeps_them() {
        let mut cfg = Config {
            pins: vec!["C:\\a\\".into(), "C:\\a".into(), " ".into()],
            ..Config::default()
        };
        cfg.sanitize();
        assert_eq!(cfg.pins, ["C:\\a"]);
        cfg.push_recent("C:\\a");
        cfg.set_recent_size("C:\\a", 42);
        assert_eq!(cfg.known_size("C:\\a\\"), 42);
        cfg.replace(Config::default());
        assert_eq!(cfg.pins, ["C:\\a"]);
    }

    #[test]
    fn backend_is_sanitized() {
        let mut cfg = Config {
            backend: "mft".into(),
            ..Config::default()
        };
        cfg.sanitize();
        assert_eq!(cfg.backend_pref(), BackendPref::Mft);
        cfg.backend = "turbo".into();
        cfg.sanitize();
        assert_eq!(cfg.backend_pref(), BackendPref::Auto);
    }

    #[test]
    fn file_colors_read_old_flag() {
        let cfg: Config = toml::from_str("class_colors = true").expect("old flag");
        assert_eq!(cfg.file_colors, FileColors::Class);
        let cfg: Config = toml::from_str("file_colors = \"age\"").expect("mode");
        assert_eq!(cfg.file_colors, FileColors::Age);
        let text = toml::to_string(&cfg).expect("save");
        assert!(text.contains("file_colors = \"age\""));
    }

    #[test]
    fn schedule_is_sanitized_and_planned() {
        let mut s = Schedule {
            every: "hourly".into(),
            day: "Wed".into(),
            time: "25:00".into(),
            roots: vec![
                "C:\\".into(),
                "c:\\".into(),
                "D:\\data\\".into(),
                " ".into(),
            ],
            ..Schedule::default()
        };
        s.sanitize();
        assert_eq!((s.every.as_str(), s.day.as_str()), ("daily", "wed"));
        assert_eq!(s.time, "12:00");
        assert_eq!(s.roots, ["C:\\", "D:\\data"]);
        s.every = "weekly".into();
        let p = s.plan("x.exe").expect("plan");
        assert_eq!((p.weekday, p.hour, p.minute), (Some(2), 12, 0));
        s.roots.clear();
        assert!(s.plan("x.exe").is_err());
    }

    #[test]
    fn replace_keeps_schedule() {
        let mut cfg = Config::default();
        cfg.schedule.enabled = true;
        cfg.replace(Config::default());
        assert!(cfg.schedule.enabled);
    }

    #[test]
    fn default_classes_round_trip() {
        let classes = Config::default().file_classes();
        assert_eq!(
            classes.classes().len(),
            FileClasses::default().classes().len()
        );
        assert_eq!(
            classes.class_of("a.mkv"),
            FileClasses::default().class_of("a.mkv")
        );
    }
}
