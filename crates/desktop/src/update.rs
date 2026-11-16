//! Update check and install through `tauri-plugin-updater`. Off while no public key is configured.

use crate::channel::Channel;
use crate::state::AppState;
use crate::{log, paths};
use semver::Version;
use serde::Serialize;
use tauri::{AppHandle, Manager, Url};
use tauri_plugin_updater::{Update, UpdaterExt};

/// Outcome of an update check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum State {
    /// No public key in `plugins.updater.pubkey`: updates are disabled.
    Off,
    /// The running version is the latest.
    Latest,
    /// A newer version exists.
    Available,
    /// The check failed; the reason is in the log.
    Failed,
}

/// What the About section and the status bar show.
#[derive(Debug, Clone, Serialize)]
pub struct UpdateInfo {
    pub state: State,
    pub current: &'static str,
    /// Version found, when one is available.
    pub version: Option<String>,
    /// The version found is older than the running one (switching to a more stable channel).
    pub downgrade: bool,
    /// Whether Install can update in place; false in portable mode, which only links to the release.
    pub installable: bool,
}

impl UpdateInfo {
    fn new(state: State, version: Option<String>) -> Self {
        let downgrade = version
            .as_deref()
            .and_then(|v| Version::parse(v).ok())
            .is_some_and(|v| v < current());
        UpdateInfo {
            state,
            current: env!("CARGO_PKG_VERSION"),
            version,
            downgrade,
            installable: state == State::Available && !paths::is_portable(),
        }
    }
}

fn current() -> Version {
    Version::parse(env!("CARGO_PKG_VERSION")).expect("package version is semver")
}

/// The updater public key from the bundled configuration, if any.
fn pubkey(app: &AppHandle) -> Option<String> {
    let key = app
        .config()
        .plugins
        .0
        .get("updater")?
        .get("pubkey")?
        .as_str()?;
    let key = key.trim();
    (!key.is_empty()).then(|| key.to_string())
}

/// Whether updates are configured in this build.
pub fn enabled(app: &AppHandle) -> bool {
    pubkey(app).is_some()
}

fn channel(app: &AppHandle) -> Channel {
    Channel::parse(&app.state::<AppState>().lock().cfg.update_channel)
}

async fn find_at(app: &AppHandle, channel: Channel, url: &str) -> Result<Option<Update>, String> {
    let url = Url::parse(url).map_err(|e| e.to_string())?;
    let updater = app
        .updater_builder()
        .endpoints(vec![url])
        .map_err(|e| e.to_string())?
        .version_comparator(move |current, remote| channel.accepts(&current, &remote.version))
        .build()
        .map_err(|e| e.to_string())?;
    updater.check().await.map_err(|e| e.to_string())
}

/// The highest version offered by the manifests of the selected channel.
/// A missing channel manifest is logged; only the stable one failing fails the check.
async fn find(app: &AppHandle) -> Result<Option<Update>, String> {
    let channel = channel(app);
    let mut best: Option<(Version, Update)> = None;
    for (i, url) in channel.endpoints().iter().enumerate() {
        match find_at(app, channel, url).await {
            Ok(Some(u)) => {
                let Ok(v) = Version::parse(&u.version) else {
                    continue;
                };
                if best.as_ref().is_none_or(|(b, _)| v > *b) {
                    best = Some((v, u));
                }
            }
            Ok(None) => {}
            Err(e) if i == 0 => return Err(e),
            Err(e) => log::warn("update", format!("{url}: {e}")),
        }
    }
    Ok(best.map(|(_, u)| u))
}

/// Asks the release endpoints for another version. Failures are logged, not returned.
pub async fn check(app: &AppHandle) -> UpdateInfo {
    if !enabled(app) {
        return UpdateInfo::new(State::Off, None);
    }
    match find(app).await {
        Ok(Some(u)) => {
            log::info("update", format!("Version {} available", u.version));
            UpdateInfo::new(State::Available, Some(u.version))
        }
        Ok(None) => UpdateInfo::new(State::Latest, None),
        Err(e) => {
            log::warn("update", format!("Check failed: {e}"));
            UpdateInfo::new(State::Failed, None)
        }
    }
}

/// Downloads and verifies the update, then runs the installer in passive mode, which restarts the app.
pub async fn install(app: &AppHandle) -> Result<(), String> {
    if !enabled(app) {
        return Err("Updates are off".into());
    }
    if paths::is_portable() {
        return Err("Portable builds update by hand".into());
    }
    let update = find(app).await?.ok_or("No update")?;
    log::info("update", format!("Installing {}", update.version));
    update
        .download_and_install(|_, _| {}, || {})
        .await
        .map_err(|e| format!("Install failed: {e}"))
}
