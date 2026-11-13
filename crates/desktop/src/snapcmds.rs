//! Tauri commands for snapshots and compare mode.

use crate::commands::{NO_SESSION, Res};
use crate::compare::Base;
use crate::fmt;
use crate::snapstore::{self, Entry};
use crate::state::{AppState, Status, TabId, snapshot_meta, status};
use diskmap_core::snapshot::SnapshotMeta;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tauri::State;

/// One stored snapshot for the compare menu.
#[derive(Serialize)]
pub struct SnapRow {
    path: String,
    date: String,
    /// Scanned size of the root.
    size: String,
    /// Size of the file.
    file: String,
    kept: bool,
    /// Taken at the end of the scan shown in the tab, so equal to it.
    this_scan: bool,
    /// The tab is compared against it.
    active: bool,
}

/// What the snapshot list of a tab needs from its session.
struct TabInfo {
    root: String,
    started: i64,
    active: Option<PathBuf>,
}

fn tab_info(state: &State<'_, AppState>, tab: TabId) -> Res<TabInfo> {
    let g = state.lock();
    let s = g.sessions.get(&tab).ok_or(NO_SESSION)?;
    Ok(TabInfo {
        root: s.scan.root_path.clone(),
        started: s.started,
        active: s.compare.as_ref().map(|c| c.base.path.clone()),
    })
}

fn row(e: Entry, info: &TabInfo) -> SnapRow {
    SnapRow {
        path: e.path.to_string_lossy().into_owned(),
        date: fmt::date_time(e.info.meta.scan_time),
        size: fmt::size(e.info.allocated),
        file: fmt::size(e.bytes),
        kept: e.kept,
        this_scan: e.info.meta.scan_time == info.started,
        active: info.active.as_ref() == Some(&e.path),
    }
}

async fn rows(info: TabInfo) -> Res<Vec<SnapRow>> {
    let root = info.root.clone();
    let list = tauri::async_runtime::spawn_blocking(move || snapstore::list(&root))
        .await
        .map_err(|e| e.to_string())?;
    Ok(list.into_iter().map(|e| row(e, &info)).collect())
}

/// Snapshots of the root of a tab, newest first.
#[tauri::command]
pub async fn snapshots(state: State<'_, AppState>, tab: TabId) -> Res<Vec<SnapRow>> {
    rows(tab_info(&state, tab)?).await
}

/// Saves a snapshot of the tab as it is now, kept from pruning; returns the new list.
#[tauri::command]
pub async fn save_snapshot(state: State<'_, AppState>, tab: TabId) -> Res<Vec<SnapRow>> {
    let (tree, meta) = {
        let g = state.lock();
        let s = g.sessions.get(&tab).ok_or(NO_SESSION)?;
        if s.scan.running {
            return Err("Scan running".into());
        }
        let meta = SnapshotMeta {
            scan_time: fmt::filetime_now(),
            ..snapshot_meta(s)
        };
        (s.scan.tree().clone(), meta)
    };
    tauri::async_runtime::spawn_blocking(move || snapstore::save(&tree, &meta, true))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| format!("Cannot save snapshot: {e}"))?;
    rows(tab_info(&state, tab)?).await
}

/// Deletes a stored snapshot.
#[tauri::command]
pub async fn delete_snapshot(path: String) -> Res<()> {
    Ok(snapstore::delete(Path::new(&path))?)
}

/// Compares a tab against a stored snapshot of its root, or leaves compare mode with `None`.
#[tauri::command]
pub async fn compare(state: State<'_, AppState>, tab: TabId, path: Option<String>) -> Res<Status> {
    let base = match path {
        Some(p) => Some(load_base(PathBuf::from(p)).await?),
        None => None,
    };
    let mut g = state.lock();
    let classes = g.classes.clone();
    let s = g.sessions.get_mut(&tab).ok_or(NO_SESSION)?;
    if let Some(b) = &base {
        if s.scan.running {
            return Err("Scan running".into());
        }
        if !same_root(b.tree.name(b.tree.root()), &s.scan.root_path) {
            return Err("Snapshot of another folder".into());
        }
    }
    s.set_compare(base, &classes);
    Ok(status(&g, tab))
}

async fn load_base(path: PathBuf) -> Res<Base> {
    if !snapstore::is_snapshot(&path) {
        return Err(format!("Not a snapshot: {}", path.display()).into());
    }
    let p = path.clone();
    let (tree, meta) = tauri::async_runtime::spawn_blocking(move || snapstore::load(&p))
        .await
        .map_err(|e| e.to_string())??;
    Ok(Base {
        tree: Arc::new(tree),
        path,
        time: meta.scan_time,
    })
}

fn same_root(a: &str, b: &str) -> bool {
    let trim = |s: &str| s.trim_end_matches(['\\', '/']).to_lowercase();
    trim(a) == trim(b)
}
