//! Live updates: one file-system watcher per tab and a worker that applies its events to the tree.

use crate::log;
use crate::state::{AppState, Inner, TabId, status};
use crossbeam_channel::{Receiver, unbounded};
use diskmap_scan::{LiveUpdater, WatchEvent, Watcher};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};

/// Events arriving within this time of the first one are applied together.
const BATCH: Duration = Duration::from_millis(250);
const MAX_BATCH: usize = 8192;
/// Events applied per lock hold, so the layout can get in between.
const CHUNK: usize = 512;
const CHUNK_GAP: Duration = Duration::from_millis(1);
/// Cluster size assumed when the volume did not report one.
pub const DEFAULT_CLUSTER: u64 = 4096;

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// A running watcher. Dropping it stops the watcher, which ends its worker.
pub struct Live {
    id: u64,
    _watcher: Watcher,
}

// SAFETY: the watcher only holds kernel handles, which any thread may use or close.
unsafe impl Send for Live {}

impl Live {
    /// Identifies this watcher among the ones a tab has had.
    pub fn id(&self) -> u64 {
        self.id
    }
}

/// Root and cluster size of a tab that should start watching now.
fn target(state: &AppState, tab: TabId, epoch: u64) -> Option<(String, u64)> {
    let g = state.lock();
    let s = g.sessions.get(&tab)?;
    let ready =
        g.cfg.watch && s.epoch == epoch && !s.scan.running && !s.scan.cancelled && s.live.is_none();
    ready.then(|| {
        (
            s.scan.root_path.clone(),
            s.scan
                .volume
                .as_ref()
                .map_or(DEFAULT_CLUSTER, |v| v.cluster_size),
        )
    })
}

/// Starts watching the finished scan of a tab when `watch` is on.
///
/// Changes made while the scan was running are not applied: the scan covers most of them and a
/// rescan covers the rest.
pub fn attach(app: &AppHandle, state: &AppState, tab: TabId, epoch: u64) {
    let Some((root, cluster)) = target(state, tab, epoch) else {
        return;
    };
    let (tx, rx) = unbounded();
    let watcher = Watcher::start(PathBuf::from(root), tx);
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    {
        let mut g = state.lock();
        let Some(s) = g
            .sessions
            .get_mut(&tab)
            .filter(|s| s.epoch == epoch && s.live.is_none())
        else {
            return;
        };
        s.live = Some(Live {
            id,
            _watcher: watcher,
        });
    }
    let (app, state) = (app.clone(), state.clone());
    std::thread::spawn(move || run(&app, &state, tab, epoch, id, &rx, cluster));
}

/// Follows the `watch` setting: stops every watcher when it is off, starts the missing ones when it is on.
pub fn sync(app: &AppHandle, state: &AppState) {
    let starts: Vec<(TabId, u64)> = {
        let mut g = state.lock();
        let on = g.cfg.watch;
        let mut starts = Vec::new();
        for (&tab, s) in &mut g.sessions {
            if on && s.live.is_none() && !s.scan.running && !s.scan.cancelled {
                starts.push((tab, s.epoch));
            } else if !on {
                s.live = None;
                s.stale = false;
            }
        }
        starts
    };
    for (tab, epoch) in starts {
        attach(app, state, tab, epoch);
    }
}

fn run(
    app: &AppHandle,
    state: &AppState,
    tab: TabId,
    epoch: u64,
    id: u64,
    rx: &Receiver<WatchEvent>,
    cluster: u64,
) {
    let mut updater = LiveUpdater::new(cluster);
    while let Some(events) = next_batch(rx) {
        if !apply(app, state, (tab, epoch, id), &mut updater, &events) {
            return;
        }
    }
}

/// Blocks for the first event, then collects whatever follows within [`BATCH`]; `None` once the watcher is gone.
fn next_batch(rx: &Receiver<WatchEvent>) -> Option<Vec<WatchEvent>> {
    let mut batch = vec![rx.recv().ok()?];
    let deadline = Instant::now() + BATCH;
    while batch.len() < MAX_BATCH {
        match rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            Ok(e) => batch.push(e),
            Err(_) => break,
        }
    }
    Some(batch)
}

/// Applies a batch in short lock holds and tells the front end; false when the watcher is no longer the tab's.
fn apply(
    app: &AppHandle,
    state: &AppState,
    (tab, epoch, id): (TabId, u64, u64),
    updater: &mut LiveUpdater,
    events: &[WatchEvent],
) -> bool {
    for (i, chunk) in events.chunks(CHUNK).enumerate() {
        if i > 0 {
            std::thread::sleep(CHUNK_GAP);
        }
        let mut g = state.lock();
        let Inner {
            sessions, classes, ..
        } = &mut *g;
        let current = sessions
            .get_mut(&tab)
            .filter(|s| s.epoch == epoch && s.live.as_ref().is_some_and(|l| l.id() == id));
        let Some(s) = current else { return false };
        let was_stale = s.stale;
        s.apply_live(updater, chunk, classes);
        if s.stale && !was_stale {
            log::warn(
                "watch",
                format!("{}: changes lost, rescan due", s.scan.root_path),
            );
        }
    }
    let snapshot = status(&state.lock(), tab);
    let _ = app.emit("scan", snapshot);
    true
}
