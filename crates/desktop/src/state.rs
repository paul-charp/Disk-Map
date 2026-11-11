//! Shared application state, the status snapshot and the scan worker thread.

use crate::config::Config;
use crate::fmt;
use crate::scan::Scan;
use crate::session::{Restore, Session, ViewOpts};
use crate::watch;
use crossbeam_channel::{Receiver, RecvTimeoutError};
use diskmap_core::classes::FileClasses;
use diskmap_core::tree::NodeId;
use diskmap_scan::ScanEvent;
use serde::Serialize;
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};

const EMIT_EVERY: Duration = Duration::from_millis(100);
const BATCH_BUDGET: Duration = Duration::from_millis(8);

/// Identifier of a tab, chosen by the front end.
pub type TabId = u32;

/// Everything behind the state mutex.
pub struct Inner {
    pub cfg: Config,
    pub classes: FileClasses,
    /// One session per tab that has a scan.
    pub sessions: HashMap<TabId, Session>,
    next_epoch: u64,
}

/// Tauri-managed handle to the shared state.
#[derive(Clone)]
pub struct AppState(pub Arc<Mutex<Inner>>);

impl Inner {
    /// Scan roots of the open tabs in tab order, the active tab last.
    pub fn tab_paths(&self, active: TabId) -> Vec<String> {
        let mut tabs: Vec<_> = self
            .sessions
            .iter()
            .map(|(&tab, s)| (tab == active, tab, s.scan.root_path.clone()))
            .collect();
        tabs.sort_by_key(|&(is_active, tab, _)| (is_active, tab));
        tabs.into_iter().map(|(_, _, path)| path).collect()
    }
}

impl AppState {
    /// Creates the state around a loaded configuration.
    pub fn new(cfg: Config) -> Self {
        AppState(Arc::new(Mutex::new(Inner {
            classes: cfg.file_classes(),
            cfg,
            sessions: HashMap::new(),
            next_epoch: 0,
        })))
    }

    /// Locks the state, ignoring poisoning.
    pub fn lock(&self) -> MutexGuard<'_, Inner> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// One breadcrumb segment.
#[derive(Serialize, Clone)]
pub struct Crumb {
    pub id: u32,
    pub name: String,
}

/// Capacity of the volume holding the scan root.
#[derive(Serialize, Clone)]
pub struct VolumeStatus {
    pub root: String,
    pub percent: u32,
}

/// Snapshot of the session for the chrome.
#[derive(Serialize, Default, Clone)]
pub struct Status {
    pub tab: TabId,
    pub active: bool,
    /// Identifies the scan; grows with every scan started, so a stale snapshot can be told apart.
    pub epoch: u64,
    pub path: String,
    pub running: bool,
    pub cancelled: bool,
    /// Grows whenever the map has to be laid out again.
    pub ver: u64,
    pub files: u64,
    pub dirs: u64,
    pub scanned: String,
    pub elapsed_ms: u64,
    pub errors: u64,
    pub error_samples: Vec<String>,
    pub volume: Option<VolumeStatus>,
    pub root: u32,
    pub crumbs: Vec<Crumb>,
    pub view_size: String,
    pub selection: Option<u32>,
    pub can_back: bool,
    pub can_fwd: bool,
    pub can_up: bool,
    pub can_rescan_view: bool,
    pub refreshing: bool,
    pub matched: u64,
    pub filter_text: String,
    pub detail: u32,
    pub class_colors: bool,
    pub show_free: bool,
    pub show_unknown: bool,
    /// `MFT` or `Walk`.
    pub backend: &'static str,
    /// Why the MFT backend is not in use, empty when it is.
    pub backend_tip: &'static str,
    pub can_elevate: bool,
    /// The watcher lost changes, so a rescan is due.
    pub stale: bool,
}

fn volume_status(s: &Session) -> Option<VolumeStatus> {
    let vol = s.scan.volume.as_ref().filter(|v| v.is_volume_root)?;
    let used = vol.total_bytes.saturating_sub(vol.free_bytes).max(1);
    let scanned = s.scan.tree().get(s.scan.tree().root()).allocated;
    let pct = (scanned.saturating_mul(100) / used).min(100) as u32;
    Some(VolumeStatus {
        root: vol.root.clone(),
        percent: if s.scan.running { pct.min(99) } else { 100 },
    })
}

fn crumbs(s: &Session) -> Vec<Crumb> {
    let tree = s.scan.tree();
    let mut out = Vec::new();
    let mut cur = Some(s.root);
    while let Some(id) = cur {
        out.push(Crumb {
            id: id.0,
            name: tree.name(id).to_string(),
        });
        cur = tree.parent(id);
    }
    out.reverse();
    out
}

/// Builds the status snapshot of a tab; inactive when the tab has no scan.
pub fn status(inner: &Inner, tab: TabId) -> Status {
    let Some(s) = inner.sessions.get(&tab) else {
        return Status {
            tab,
            ..Status::default()
        };
    };
    Status {
        tab,
        active: true,
        epoch: s.epoch,
        path: s.scan.root_path.clone(),
        running: s.scan.running,
        cancelled: s.scan.cancelled,
        ver: s.scan.version + s.filter.ver + s.tags_ver,
        files: s.scan.tree().file_count(),
        dirs: s.scan.tree().dir_count(),
        scanned: fmt::size(s.scan.tree().get(s.scan.tree().root()).allocated),
        elapsed_ms: s.scan.elapsed().as_millis() as u64,
        errors: s.scan.errors,
        error_samples: s.scan.error_samples.clone(),
        volume: volume_status(s),
        root: s.root.0,
        crumbs: crumbs(s),
        view_size: fmt::size(s.size_of(s.root)),
        selection: s.selection.map(|n| n.0),
        can_back: s.can_back(),
        can_fwd: s.can_forward(),
        can_up: s.can_up(),
        can_rescan_view: s.can_rescan_view(),
        refreshing: s.refreshing,
        matched: s.matched_files(),
        filter_text: s.filter.text.clone(),
        detail: s.view.detail,
        class_colors: s.view.class_colors,
        show_free: s.view.show_free,
        show_unknown: s.view.show_unknown,
        backend: s.scan.backend.name(),
        backend_tip: s.scan.backend_tip(),
        can_elevate: s.scan.can_elevate(),
        stale: s.stale,
    }
}

/// Starts scanning `path` in a tab, replacing its session. A rescan keeps the view options and `restore`.
pub fn start_scan(
    app: &AppHandle,
    state: &AppState,
    tab: TabId,
    path: &str,
    carry: Option<(ViewOpts, Restore)>,
) -> Result<(), String> {
    let path = Path::new(path.trim());
    if !path.is_dir() {
        return Err("Not a folder".into());
    }
    let (threads, pref) = {
        let g = state.lock();
        (g.cfg.scan_threads(), g.cfg.backend_pref())
    };
    let (scan, rx) = Scan::start(path, threads, pref);
    let epoch = {
        let mut g = state.lock();
        g.next_epoch += 1;
        let epoch = g.next_epoch;
        let root = scan.root_path.clone();
        g.cfg.push_recent(&root);
        g.cfg.save();
        let (view, restore) = carry.map_or_else(
            || (ViewOpts::from_config(&g.cfg), None),
            |(v, r)| (v, Some(r)),
        );
        let mut session = Session::new(epoch, scan, view);
        if let Some(r) = restore {
            session.restore_after_scan(r, &g.classes);
        }
        g.sessions.insert(tab, session);
        epoch
    };
    let (app, state) = (app.clone(), state.clone());
    std::thread::spawn(move || run_worker(&app, &state, &rx, tab, epoch));
    Ok(())
}

/// Scans the root of a tab again, keeping its view options, filter, view root and selection.
pub fn start_rescan(app: &AppHandle, state: &AppState, tab: TabId) -> Result<(), String> {
    let (path, carry) = {
        let g = state.lock();
        let s = g.sessions.get(&tab).ok_or("No scan")?;
        (s.scan.root_path.clone(), (s.view, s.remember()))
    };
    start_scan(app, state, tab, &path, Some(carry))
}

/// Rescans the view root alone and swaps the result into the tree when it completes.
pub fn start_view_rescan(app: &AppHandle, state: &AppState, tab: TabId) -> Result<(), String> {
    let (path, node, epoch, threads, pref) = {
        let mut g = state.lock();
        let (threads, pref) = (g.cfg.scan_threads(), g.cfg.backend_pref());
        let s = g
            .sessions
            .get_mut(&tab)
            .filter(|s| s.can_rescan_view())
            .ok_or("Cannot rescan view")?;
        s.refreshing = true;
        (
            s.scan.tree().full_path(s.root),
            s.root,
            s.epoch,
            threads,
            pref,
        )
    };
    let (mut sub, rx) = Scan::start(Path::new(&path), threads, pref);
    let (app, state) = (app.clone(), state.clone());
    std::thread::spawn(move || {
        while let Ok(event) = rx.recv() {
            if sub.apply(event) {
                break;
            }
        }
        let snapshot = {
            let mut g = state.lock();
            let Some(s) = g.sessions.get_mut(&tab).filter(|s| s.epoch == epoch) else {
                return;
            };
            s.refreshing = false;
            if !sub.cancelled {
                s.scan.replace_subtree(node, sub.tree());
                s.selection = None;
            }
            status(&g, tab)
        };
        let _ = app.emit("scan", snapshot);
    });
    Ok(())
}

fn run_worker(app: &AppHandle, state: &AppState, rx: &Receiver<ScanEvent>, tab: TabId, epoch: u64) {
    let mut last_emit = Instant::now();
    loop {
        let event = rx.recv_timeout(Duration::from_millis(40));
        let (snapshot, done) = {
            let mut g = state.lock();
            let Inner {
                sessions,
                classes,
                cfg,
                ..
            } = &mut *g;
            let Some(s) = sessions.get_mut(&tab).filter(|s| s.epoch == epoch) else {
                return;
            };
            let done = pump(s, event, rx);
            if done {
                s.finish_restore();
            }
            s.refresh_filter(classes);
            if done {
                let (root, size) = (
                    s.scan.root_path.clone(),
                    s.scan.tree().get(s.scan.tree().root()).allocated,
                );
                cfg.set_recent_size(&root, size);
                cfg.save();
            }
            let due = done || last_emit.elapsed() >= EMIT_EVERY;
            (due.then(|| status(&g, tab)), done)
        };
        if let Some(snapshot) = snapshot {
            let _ = app.emit("scan", snapshot);
            last_emit = Instant::now();
        }
        if done {
            watch::attach(app, state, tab, epoch);
            return;
        }
    }
}

/// Applies the received event and whatever else is queued within the budget; true when the scan ended.
fn pump(
    s: &mut Session,
    first: Result<ScanEvent, RecvTimeoutError>,
    rx: &Receiver<ScanEvent>,
) -> bool {
    let event = match first {
        Ok(e) => e,
        Err(RecvTimeoutError::Timeout) => return false,
        Err(RecvTimeoutError::Disconnected) => {
            s.scan.finish();
            return true;
        }
    };
    if s.scan.apply(event) {
        return true;
    }
    let start = Instant::now();
    while start.elapsed() < BATCH_BUDGET {
        match rx.try_recv() {
            Ok(e) => {
                if s.scan.apply(e) {
                    return true;
                }
            }
            Err(_) => break,
        }
    }
    false
}

/// Node id for a value from the front end, if it exists in the tree.
pub fn node_id(s: &Session, raw: i32) -> Option<NodeId> {
    let id = u32::try_from(raw).ok()?;
    ((id as usize) < s.scan.tree().len()).then_some(NodeId(id))
}
