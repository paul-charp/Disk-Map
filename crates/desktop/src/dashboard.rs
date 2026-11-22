//! The `dashboard` command: everything the start screen shows, in one call.

use crate::commands::{Res, TimelinePoint, kind_name, timeline_points};
use crate::config::{Config, Schedule};
use crate::snapstore;
use crate::state::AppState;
use crate::timeline::{self, Mover};
use diskmap_core::key::root_key;
use diskmap_core::report::{report, window_start};
use diskmap_scan::volume::list_drives;
use serde::Serialize;
use std::collections::HashSet;
use tauri::State;

/// Growers and shrinkers listed per root.
const MOVERS: usize = 5;
/// Window of the movers when the front end gives none, in days.
const DEFAULT_DAYS: i64 = 7;

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

/// Change of a root over the movers window. Times are Unix seconds; sizes are allocated bytes.
#[derive(Serialize)]
pub struct MoverCard {
    root: String,
    from: i64,
    to: i64,
    old: u64,
    new: u64,
    growers: Vec<Mover>,
    shrinkers: Vec<Mover>,
    /// The root has a snapshot to compare with.
    snapshot: bool,
}

/// Start screen content.
#[derive(Serialize)]
pub struct Dashboard {
    drives: Vec<DriveCard>,
    pins: Vec<PathCard>,
    recent: Vec<PathCard>,
    movers: Vec<MoverCard>,
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

/// Movers of one root between the newest point and the window start (the oldest point when the
/// history is shorter than the window); `None` without two readable points.
fn mover_card(root: &str, days: Option<i64>) -> Option<MoverCard> {
    let points = timeline::list(root);
    let times: Vec<i64> = points.iter().map(|p| p.time).collect();
    let start = window_start(&times, days).or_else(|| window_start(&times, None))?;
    let (first, last) = (&points[start], points.last()?);
    let load = |p: &timeline::Point| {
        timeline::load(&p.path)
            .map_err(|e| crate::log::warn("timeline", e))
            .ok()
    };
    let (old, new) = (load(first)?, load(last)?);
    let r = report(&old, &new, MOVERS);
    let movers = |list: Vec<diskmap_core::report::Mover>| {
        list.into_iter()
            .map(|m| Mover {
                path: m.path,
                delta: m.delta,
            })
            .collect()
    };
    Some(MoverCard {
        root: new.root_path.clone(),
        from: first.time,
        to: last.time,
        old: r.old_total,
        new: r.new_total,
        growers: movers(r.growers),
        shrinkers: movers(r.shrinkers),
        snapshot: !snapstore::list(root).is_empty(),
    })
}

/// One card per root with history among the drives, scheduled roots and pins, largest change first.
fn mover_cards(cfg: &Config, drives: &[DriveCard], days: Option<i64>) -> Vec<MoverCard> {
    let mut seen = HashSet::new();
    let roots = drives
        .iter()
        .map(|d| d.root.as_str())
        .chain(cfg.schedule.roots.iter().map(String::as_str))
        .chain(cfg.pins.iter().map(String::as_str));
    let mut cards: Vec<MoverCard> = roots
        .filter(|r| seen.insert(root_key(r)))
        .filter_map(|r| mover_card(r, days))
        .collect();
    cards.sort_by_key(|c| std::cmp::Reverse((c.new as i128 - c.old as i128).unsigned_abs()));
    cards
}

fn build(cfg: &Config, days: Option<i64>) -> Dashboard {
    let drives = drive_cards();
    Dashboard {
        movers: mover_cards(cfg, &drives, days),
        drives,
        pins: cfg.pins.iter().map(|p| path_card(cfg, p, true)).collect(),
        recent: cfg
            .recent
            .iter()
            .map(|r| path_card(cfg, &r.path, false))
            .collect(),
        schedule: cfg.schedule.clone(),
        admin: diskmap_scan::is_elevated(),
    }
}

/// Drives with their timelines, pins, recent scans, the movers over the last `days` (0 for all
/// the history, default 7) and the schedule.
#[tauri::command]
pub async fn dashboard(state: State<'_, AppState>, days: Option<i64>) -> Res<Dashboard> {
    let cfg = state.lock().cfg.clone();
    let days = match days.unwrap_or(DEFAULT_DAYS) {
        0 => None,
        d => Some(d),
    };
    Ok(
        tauri::async_runtime::spawn_blocking(move || build(&cfg, days))
            .await
            .map_err(|e| e.to_string())?,
    )
}
