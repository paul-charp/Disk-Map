//! The `dashboard` command: everything the start screen shows, in one call.

use crate::commands::{Res, TimelinePoint, kind_name, timeline_points};
use crate::config::{Config, Schedule};
use crate::snapstore;
use crate::state::AppState;
use crate::timeline::{self, ReportLine};
use diskmap_scan::volume::list_drives;
use serde::Serialize;
use tauri::State;

const REPORTS: usize = 10;

/// A mounted drive with its timeline. Sizes are bytes.
#[derive(Serialize)]
pub struct DriveCard {
    root: String,
    label: String,
    kind: &'static str,
    file_system: String,
    /// The MFT backend can read it when elevated.
    mft: bool,
    total: u64,
    free: u64,
    points: Vec<TimelinePoint>,
}

/// A pinned folder or recent scan.
#[derive(Serialize)]
pub struct PathCard {
    path: String,
    /// Allocated bytes of the last scan, 0 when unknown.
    size: u64,
    pinned: bool,
    /// Last scan or timeline point, Unix seconds.
    time: Option<i64>,
    /// Change between the last two timeline points.
    delta: Option<i64>,
}

/// A report line and whether its root has a snapshot to compare with.
#[derive(Serialize)]
pub struct ReportCard {
    #[serde(flatten)]
    line: ReportLine,
    snapshot: bool,
}

/// Start screen content.
#[derive(Serialize)]
pub struct Dashboard {
    drives: Vec<DriveCard>,
    pins: Vec<PathCard>,
    recent: Vec<PathCard>,
    reports: Vec<ReportCard>,
    schedule: Schedule,
    /// This process runs as administrator.
    admin: bool,
}

fn path_card(cfg: &Config, path: &str, pinned: bool) -> PathCard {
    let points = timeline_points(path);
    let known = cfg.known(path);
    let last = points.last().map(|p| p.time);
    let time = known.map(|r| r.time).filter(|&t| t > 0).max(last);
    let delta = match points.as_slice() {
        [.., a, b] => Some(b.allocated as i64 - a.allocated as i64),
        _ => None,
    };
    PathCard {
        path: path.to_string(),
        size: known.map_or(0, |r| r.size),
        pinned: pinned || cfg.is_pinned(path),
        time,
        delta,
    }
}

fn drive_cards() -> Vec<DriveCard> {
    list_drives()
        .into_iter()
        .map(|d| DriveCard {
            mft: d.mft_capable(),
            kind: kind_name(d.kind),
            points: timeline_points(&d.root),
            root: d.root,
            label: d.label,
            file_system: d.file_system,
            total: d.total_bytes,
            free: d.free_bytes,
        })
        .collect()
}

fn report_cards() -> Vec<ReportCard> {
    timeline::reports(REPORTS)
        .into_iter()
        .map(|line| ReportCard {
            snapshot: !snapstore::list(&line.root).is_empty(),
            line,
        })
        .collect()
}

fn build(cfg: &Config) -> Dashboard {
    Dashboard {
        drives: drive_cards(),
        pins: cfg.pins.iter().map(|p| path_card(cfg, p, true)).collect(),
        recent: cfg
            .recent
            .iter()
            .map(|r| path_card(cfg, &r.path, false))
            .collect(),
        reports: report_cards(),
        schedule: cfg.schedule.clone(),
        admin: diskmap_scan::is_elevated(),
    }
}

/// Drives with their timelines, pins, recent scans, the latest reports and the schedule.
#[tauri::command]
pub async fn dashboard(state: State<'_, AppState>) -> Res<Dashboard> {
    let cfg = state.lock().cfg.clone();
    Ok(tauri::async_runtime::spawn_blocking(move || build(&cfg))
        .await
        .map_err(|e| e.to_string())?)
}
