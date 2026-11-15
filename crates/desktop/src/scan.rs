//! Scan lifecycle: scanner thread, event application and the growing tree.

use crate::config::BackendPref;
use crate::log;
use crossbeam_channel::{Receiver, unbounded};
use diskmap_core::tree::{NodeId, Tree};
use diskmap_scan::volume::{VolumeInfo, volume_info};
use diskmap_scan::walk::absolute_root;
use diskmap_scan::{
    Backend, MftSupport, ScanEvent, Scanner, TreeBuilder, WalkScanner, auto_scanner, mft_available,
};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

const ERROR_SAMPLES: usize = 5;

/// A running or finished scan and the tree built from it.
pub struct Scan {
    pub root_path: String,
    pub running: bool,
    pub cancelled: bool,
    pub errors: u64,
    /// The first few errors, as `path (code)`.
    pub error_samples: Vec<String>,
    started: Instant,
    elapsed: Option<Duration>,
    /// Incremented whenever the tree changed.
    pub version: u64,
    pub volume: Option<VolumeInfo>,
    /// Backend the scan runs on.
    pub backend: Backend,
    pref: BackendPref,
    mft: MftSupport,
    builder: TreeBuilder,
    cancel: Arc<AtomicBool>,
}

impl Scan {
    /// Starts scanning `path` on `threads` workers (automatic when `None`); events arrive on the receiver.
    pub fn start(
        path: &Path,
        threads: Option<usize>,
        pref: BackendPref,
    ) -> (Scan, Receiver<ScanEvent>) {
        let root_path = absolute_root(path);
        let (tx, rx) = unbounded();
        let cancel = Arc::new(AtomicBool::new(false));
        let (scanner, backend) = pick_scanner(path, threads, pref);
        scanner.start(path.to_path_buf(), tx, cancel.clone());
        let mft = mft_available(path);
        log_fallback(&root_path, pref, backend, mft);
        let scan = Scan {
            volume: volume_info(path).ok(),
            backend,
            pref,
            mft,
            builder: TreeBuilder::new(&root_path),
            root_path,
            running: true,
            cancelled: false,
            errors: 0,
            error_samples: Vec::new(),
            started: Instant::now(),
            elapsed: None,
            version: 0,
            cancel,
        };
        (scan, rx)
    }

    /// The tree built so far.
    pub fn tree(&self) -> &Tree {
        self.builder.tree()
    }

    /// Mutable access for live updates; call [`Scan::touch`] afterwards.
    pub fn tree_mut(&mut self) -> &mut Tree {
        self.builder.tree_mut()
    }

    /// Notes that the tree changed.
    pub fn touch(&mut self) {
        self.version += 1;
    }

    /// Why the MFT backend is not in use, empty when it is or when the walk was chosen.
    pub fn backend_tip(&self) -> &'static str {
        if self.backend == Backend::Walk && self.pref != BackendPref::Walk {
            self.mft.reason()
        } else {
            ""
        }
    }

    /// True when running as administrator would make the MFT backend available.
    pub fn can_elevate(&self) -> bool {
        self.mft == MftSupport::NotElevated
    }

    /// True when the scan ran to its end and read the root: not stopped, not failed outright.
    pub fn completed(&self) -> bool {
        let tree = self.tree();
        let empty = tree.get(tree.root()).child_count() == 0;
        !self.running && !self.cancelled && !(empty && self.errors > 0)
    }

    /// Asks the scanner to stop.
    pub fn stop(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    /// Applies one event; returns true when the scan is over.
    pub fn apply(&mut self, event: ScanEvent) -> bool {
        match event {
            ScanEvent::Batch(batch) => {
                self.builder.apply_batch(&batch);
                self.version += 1;
            }
            ScanEvent::Error { path, code } => {
                self.errors += 1;
                if self.error_samples.len() < ERROR_SAMPLES {
                    self.error_samples.push(format!("{path} ({code})"));
                }
            }
            ScanEvent::Done { cancelled } => {
                self.cancelled = cancelled;
                self.finish();
                return true;
            }
        }
        false
    }

    /// Replaces everything below `node` with the contents of a finished scan of that folder.
    pub fn replace_subtree(&mut self, node: NodeId, sub: &Tree) {
        let tree = self.builder.tree_mut();
        tree.clear_children(node);
        tree.graft_children(node, sub, sub.root());
        self.version += 1;
    }

    /// Marks the scan as over.
    pub fn finish(&mut self) {
        self.running = false;
        self.elapsed.get_or_insert_with(|| self.started.elapsed());
        self.version += 1;
    }

    /// Time spent scanning so far, or in total once finished.
    pub fn elapsed(&self) -> Duration {
        self.elapsed.unwrap_or_else(|| self.started.elapsed())
    }
}

/// Logs a scan that runs on the walk although the MFT was wanted: a warning when MFT was chosen, a note on Auto.
fn log_fallback(root: &str, pref: BackendPref, backend: Backend, mft: MftSupport) {
    if backend != Backend::Walk || pref == BackendPref::Walk {
        return;
    }
    let text = format!("MFT unavailable for {root}: {}; using Walk", mft.reason());
    if pref == BackendPref::Mft {
        log::warn("scan", text);
    } else {
        log::info("scan", text);
    }
}

fn pick_scanner(
    path: &Path,
    threads: Option<usize>,
    pref: BackendPref,
) -> (Box<dyn Scanner>, Backend) {
    match pref {
        BackendPref::Walk => (Box::new(WalkScanner::new(threads)), Backend::Walk),
        BackendPref::Auto | BackendPref::Mft => auto_scanner(path, threads),
    }
}

impl Drop for Scan {
    fn drop(&mut self) {
        self.stop();
    }
}
