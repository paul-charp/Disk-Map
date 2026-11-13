//! Config and data locations: installed under the user profile, or portable next to the exe.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

const CONFIG_FILE: &str = "diskmap.toml";
const APP_DIR: &str = "diskmap";
const DATA_DIR: &str = "data";

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
    /// Folder the installer recorded in `HKCU\Software\diskmap\InstallDir`.
    pub install_dir: Option<PathBuf>,
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
        let canon = |p: PathBuf| std::fs::canonicalize(&p).unwrap_or(p);
        Some(Env {
            exe_dir: canon(std::env::current_exe().ok()?.parent()?.to_path_buf()),
            install_dir: registry_install_dir().map(canon),
            config_override: var("DISKMAP_CONFIG"),
            appdata: var("APPDATA"),
            local_appdata: var("LOCALAPPDATA"),
        })
    }
}

#[cfg(windows)]
fn registry_install_dir() -> Option<PathBuf> {
    use std::os::windows::ffi::OsStringExt;
    use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_SZ, RegGetValueW};
    use windows::core::w;

    let mut buf = vec![0u16; 4096];
    let mut size = (buf.len() * 2) as u32;
    // SAFETY: `buf` is writable for `size` bytes; the key and value names are static wide strings.
    let err = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!(r"Software\diskmap"),
            w!("InstallDir"),
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr().cast()),
            Some(&mut size),
        )
    };
    if err.is_err() {
        return None;
    }
    let len = (size as usize / 2).saturating_sub(1);
    let dir = PathBuf::from(std::ffi::OsString::from_wide(&buf[..len]));
    (!dir.as_os_str().is_empty()).then_some(dir)
}

#[cfg(not(windows))]
fn registry_install_dir() -> Option<PathBuf> {
    None
}

fn normalized(p: &Path) -> String {
    let s = p.to_string_lossy().replace('/', "\\").to_lowercase();
    let s = s.strip_prefix(r"\\?\").unwrap_or(&s);
    s.trim_end_matches('\\').to_string()
}

/// True when both paths name the same folder, ignoring case, slashes and a trailing separator.
pub fn same_dir(a: &Path, b: &Path) -> bool {
    normalized(a) == normalized(b)
}

/// Decides the mode and the paths. Installed only when the exe runs from the folder the
/// installer recorded and the profile folders are known; portable otherwise.
pub fn resolve(env: &Env) -> Paths {
    let recorded = env
        .install_dir
        .as_deref()
        .is_some_and(|dir| same_dir(dir, &env.exe_dir));
    let installed = match (&env.appdata, &env.local_appdata) {
        (Some(roaming), Some(local)) if recorded => Some((
            roaming.join(APP_DIR).join(CONFIG_FILE),
            local.join(APP_DIR).join(DATA_DIR),
        )),
        _ => None,
    };
    let (portable, config, data) = match installed {
        Some((config, data)) => (false, config, data),
        None => (
            true,
            env.exe_dir.join(CONFIG_FILE),
            env.exe_dir.join(DATA_DIR),
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
        .get_or_init(|| Env::from_process().map(|env| resolve(&env)))
        .as_ref()
}

/// Path of `diskmap.toml`.
pub fn config_path() -> Option<PathBuf> {
    paths().map(|p| p.config.clone())
}

/// Folder for snapshots, timeline and logs. Not created until [`ensure_data_dir`].
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
pub fn is_portable() -> bool {
    paths().is_some_and(|p| p.portable)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env() -> Env {
        Env {
            exe_dir: PathBuf::from(r"C:\Users\me\AppData\Local\diskmap"),
            install_dir: Some(PathBuf::from(r"C:\Users\me\AppData\Local\diskmap")),
            config_override: None,
            appdata: Some(PathBuf::from("roaming")),
            local_appdata: Some(PathBuf::from("local")),
        }
    }

    #[test]
    fn installed_when_key_matches() {
        let p = resolve(&env());
        assert!(!p.portable);
        assert_eq!(
            p.config,
            Path::new("roaming").join("diskmap").join("diskmap.toml")
        );
        assert_eq!(p.data, Path::new("local").join("diskmap").join("data"));
    }

    #[test]
    fn portable_without_key() {
        let mut e = env();
        e.install_dir = None;
        let p = resolve(&e);
        assert!(p.portable);
        assert_eq!(p.config, e.exe_dir.join("diskmap.toml"));
        assert_eq!(p.data, e.exe_dir.join("data"));
    }

    #[test]
    fn portable_when_key_points_elsewhere() {
        let mut e = env();
        e.exe_dir = PathBuf::from(r"C:\Users\me\Desktop");
        assert!(resolve(&e).portable);
    }

    #[test]
    fn match_ignores_case_and_separators() {
        let a = Path::new(r"C:\Users\Me\AppData\Local\diskmap\");
        assert!(same_dir(a, Path::new(r"c:\users\me\appdata\local\DISKMAP")));
        assert!(same_dir(
            a,
            Path::new(r"\\?\C:\Users\me\AppData\Local\diskmap")
        ));
        assert!(same_dir(a, Path::new("C:/Users/me/AppData/Local/diskmap")));
        assert!(!same_dir(
            a,
            Path::new(r"C:\Users\me\AppData\Local\diskmap2")
        ));
    }

    #[test]
    fn override_keeps_mode() {
        let mut e = env();
        e.config_override = Some(PathBuf::from("x.toml"));
        let p = resolve(&e);
        assert!(!p.portable);
        assert_eq!(p.config, PathBuf::from("x.toml"));
        assert_eq!(p.data, Path::new("local").join("diskmap").join("data"));
    }

    #[test]
    fn missing_profile_falls_back_to_portable() {
        let mut e = env();
        e.local_appdata = None;
        assert!(resolve(&e).portable);
    }
}
