//! Config and data locations: portable next to the exe, or installed under the user profile.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

const CONFIG_FILE: &str = "diskmap.toml";
const MARKER_FILE: &str = "portable";
const APP_DIR: &str = "diskmap";
const PORTABLE_DATA_DIR: &str = "data";

/// Resolved locations, decided once at startup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
    pub portable: bool,
    pub config: PathBuf,
    pub data: PathBuf,
}

/// Inputs of the resolver, read from the process environment.
#[derive(Debug, Default)]
pub struct Env {
    pub exe_dir: PathBuf,
    pub config_override: Option<PathBuf>,
    pub appdata: Option<PathBuf>,
    pub local_appdata: Option<PathBuf>,
}

impl Env {
    fn from_process() -> Option<Self> {
        let var = |k: &str| {
            std::env::var_os(k)
                .filter(|v| !v.is_empty())
                .map(PathBuf::from)
        };
        Some(Env {
            exe_dir: std::env::current_exe().ok()?.parent()?.to_path_buf(),
            config_override: var("DISKMAP_CONFIG"),
            appdata: var("APPDATA"),
            local_appdata: var("LOCALAPPDATA"),
        })
    }
}

/// True when the exe folder holds a `portable` marker or a `diskmap.toml`.
pub fn portable_dir(exe_dir: &Path, exists: impl Fn(&Path) -> bool) -> bool {
    exists(&exe_dir.join(MARKER_FILE)) || exists(&exe_dir.join(CONFIG_FILE))
}

/// Decides the mode and the paths. Falls back to portable when the profile folders are unknown.
pub fn resolve(env: &Env, exists: impl Fn(&Path) -> bool) -> Paths {
    let installed = match (&env.appdata, &env.local_appdata) {
        (Some(roaming), Some(local)) if !portable_dir(&env.exe_dir, exists) => {
            Some((roaming.join(APP_DIR).join(CONFIG_FILE), local.join(APP_DIR)))
        }
        _ => None,
    };
    let (portable, config, data) = match installed {
        Some((config, data)) => (false, config, data),
        None => (
            true,
            env.exe_dir.join(CONFIG_FILE),
            env.exe_dir.join(PORTABLE_DATA_DIR),
        ),
    };
    Paths {
        portable,
        config: env.config_override.clone().unwrap_or(config),
        data,
    }
}

fn paths() -> Option<&'static Paths> {
    static PATHS: OnceLock<Option<Paths>> = OnceLock::new();
    PATHS
        .get_or_init(|| Env::from_process().map(|env| resolve(&env, Path::exists)))
        .as_ref()
}

/// Path of `diskmap.toml`.
pub fn config_path() -> Option<PathBuf> {
    paths().map(|p| p.config.clone())
}

/// Folder for snapshots, timeline and logs. Not created until [`ensure_data_dir`].
#[allow(dead_code)] // first users: snapshots, logs
pub fn data_dir() -> Option<PathBuf> {
    paths().map(|p| p.data.clone())
}

/// Creates the data folder if needed and returns it.
pub fn ensure_data_dir() -> std::io::Result<PathBuf> {
    let dir = data_dir().ok_or_else(|| std::io::Error::other("no data folder"))?;
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// True when running in portable mode.
#[allow(dead_code)] // first user: About
pub fn is_portable() -> bool {
    paths().is_some_and(|p| p.portable)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env() -> Env {
        Env {
            exe_dir: PathBuf::from("app"),
            config_override: None,
            appdata: Some(PathBuf::from("roaming")),
            local_appdata: Some(PathBuf::from("local")),
        }
    }

    fn only(file: &'static str) -> impl Fn(&Path) -> bool {
        move |p| p == Path::new("app").join(file)
    }

    #[test]
    fn installed_by_default() {
        let p = resolve(&env(), |_| false);
        assert!(!p.portable);
        assert_eq!(
            p.config,
            Path::new("roaming").join("diskmap").join("diskmap.toml")
        );
        assert_eq!(p.data, Path::new("local").join("diskmap"));
    }

    #[test]
    fn marker_makes_portable() {
        let p = resolve(&env(), only("portable"));
        assert!(p.portable);
        assert_eq!(p.config, Path::new("app").join("diskmap.toml"));
        assert_eq!(p.data, Path::new("app").join("data"));
    }

    #[test]
    fn existing_config_makes_portable() {
        assert!(resolve(&env(), only("diskmap.toml")).portable);
    }

    #[test]
    fn override_keeps_mode() {
        let mut e = env();
        e.config_override = Some(PathBuf::from("x.toml"));
        let p = resolve(&e, |_| false);
        assert!(!p.portable);
        assert_eq!(p.config, PathBuf::from("x.toml"));
        assert_eq!(p.data, Path::new("local").join("diskmap"));
    }

    #[test]
    fn missing_profile_falls_back_to_portable() {
        let mut e = env();
        e.local_appdata = None;
        assert!(resolve(&e, |_| false).portable);
    }
}
