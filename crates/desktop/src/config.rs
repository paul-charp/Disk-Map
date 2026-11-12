//! Settings persisted in `diskmap.toml` next to the executable (or at `DISKMAP_CONFIG`).

use crate::log;
use diskmap_core::classes::{FileClass, FileClasses};
use serde::{Deserialize, Serialize};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

const MAX_RECENT: usize = 8;
const SCALE_RANGE: (u32, u32) = (80, 200);
const MIN_TILE_RANGE: (u32, u32) = (2, 16);
const MAX_THREADS: u32 = 64;
const TEXT_SIZES: [&str; 3] = ["small", "medium", "large"];
const BACKENDS: [&str; 3] = ["auto", "mft", "walk"];

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

/// Persisted settings.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub theme: String,
    pub recent: Vec<Recent>,
    /// Defaults for new tabs.
    pub detail: u32,
    pub class_colors: bool,
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
    pub classes: Vec<ClassDef>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            theme: "graphite".into(),
            recent: Vec::new(),
            detail: 3,
            class_colors: false,
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
            classes: class_defs(&FileClasses::default()),
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

fn config_path() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("DISKMAP_CONFIG") {
        return Some(PathBuf::from(p));
    }
    Some(std::env::current_exe().ok()?.parent()?.join("diskmap.toml"))
}

/// Path of the log file, next to the configuration file.
pub fn log_path() -> Option<PathBuf> {
    Some(config_path()?.with_file_name("diskmap.log"))
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
        let mut cfg = config_path().and_then(|p| read(&p)).unwrap_or_default();
        cfg.sanitize();
        cfg
    }

    /// Writes the file; failures are logged.
    pub fn save(&self) {
        let Some(path) = config_path() else { return };
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
        if !TEXT_SIZES.contains(&self.map_text.as_str()) {
            self.map_text = "medium".into();
        }
        if !BACKENDS.contains(&self.backend.as_str()) {
            self.backend = "auto".into();
        }
        for c in &mut self.classes {
            c.name = c.name.trim().to_string();
            if c.name.is_empty() {
                c.name = "Class".into();
            }
        }
    }

    /// Replaces every setting but the recent list with those of `new`.
    pub fn replace(&mut self, mut new: Config) {
        new.recent = std::mem::take(&mut self.recent);
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
