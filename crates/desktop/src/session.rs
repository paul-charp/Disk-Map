//! State of one scan: navigation, selection, tags, filter and statistics.

use crate::config::Config;
use crate::fmt;
use crate::marks::Marks;
use crate::scan::Scan;
use crate::watch::{DEFAULT_CLUSTER, Live};
use diskmap_core::classes::FileClasses;
use diskmap_core::filter::{Filter, FilterCtx, Filtered};
use diskmap_core::layout::Sizes;
use diskmap_core::tags::{Tag, Tags};
use diskmap_core::tree::{NodeId, NodeKind, Tree};
use diskmap_scan::{Change, ChangeKind, LiveUpdater, WatchEvent, WatchKind};
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, HashSet};
use std::path::Path;
use std::time::{Duration, Instant};

const SCAN_REFRESH: Duration = Duration::from_millis(500);
const TOP_N: usize = 10;

/// Filter text, parsed filter and its evaluation.
pub struct FilterState {
    pub text: String,
    pub filter: Filter,
    pub filtered: Option<Filtered>,
    pub bold: HashSet<NodeId>,
    /// Incremented whenever the evaluation changed.
    pub ver: u64,
    dirty: bool,
    applied_tree_ver: u64,
    applied_tags_ver: u64,
    applied_at: Option<Instant>,
}

impl FilterState {
    fn new() -> Self {
        FilterState {
            text: String::new(),
            filter: Filter::void(),
            filtered: None,
            bold: HashSet::new(),
            ver: 0,
            dirty: false,
            applied_tree_ver: 0,
            applied_tags_ver: 0,
            applied_at: None,
        }
    }
}

/// Largest items and class totals under the view root.
#[derive(Default)]
pub struct Stats {
    pub files: Vec<(NodeId, u64)>,
    pub folders: Vec<(NodeId, u64)>,
    /// Bytes per class, the last entry is files without a class.
    pub classes: Vec<u64>,
    key: Option<(NodeId, u64, u64)>,
    at: Option<Instant>,
}

/// Per-tab view options, initialised from the configured defaults.
#[derive(Clone, Copy, Debug)]
pub struct ViewOpts {
    pub detail: u32,
    pub class_colors: bool,
    pub show_free: bool,
    pub show_unknown: bool,
}

impl ViewOpts {
    /// The defaults for a new tab.
    pub fn from_config(cfg: &Config) -> Self {
        ViewOpts {
            detail: cfg.detail,
            class_colors: cfg.class_colors,
            show_free: cfg.show_free,
            show_unknown: cfg.show_unknown,
        }
    }
}

/// View root and selection of a previous scan, as names below the scan root.
#[derive(Default)]
pub struct Restore {
    root: Vec<String>,
    selection: Option<Vec<String>>,
    filter: String,
}

fn names_to(tree: &Tree, mut id: NodeId) -> Vec<String> {
    let mut out = Vec::new();
    while let Some(parent) = tree.parent(id) {
        out.push(tree.name(id).to_string());
        id = parent;
    }
    out.reverse();
    out
}

/// Node reached from the tree root through `names`, matched case-insensitively.
pub fn find_by_names<S: AsRef<str>>(tree: &Tree, names: &[S]) -> Option<NodeId> {
    let mut cur = tree.root();
    for part in names {
        cur = tree
            .children(cur)
            .find(|&c| tree.name(c).eq_ignore_ascii_case(part.as_ref()))?;
    }
    Some(cur)
}

/// The deepest existing ancestor of a path relative to the scan root, excluding the path itself.
fn nearest_existing(tree: &Tree, rel: &str) -> NodeId {
    let mut path = rel;
    while let Some((parent, _)) = path.rsplit_once('\\') {
        if let Some(id) = tree.find_path(parent) {
            return id;
        }
        path = parent;
    }
    tree.root()
}

/// The node a change is shown on and whether it is a warning: removals show on the parent.
fn mark_of(change: &Change, survivors: &HashMap<NodeId, NodeId>) -> (NodeId, bool) {
    let node = survivors.get(&change.node).copied().unwrap_or(change.node);
    (
        node,
        matches!(change.kind, ChangeKind::Removed | ChangeKind::Shrank),
    )
}

/// `id` if it is still in the tree, else the surviving ancestor recorded for the removed subtree it was in.
fn survivor(tree: &Tree, id: NodeId, survivors: &HashMap<NodeId, NodeId>) -> NodeId {
    let mut top = id;
    while let Some(p) = tree.parent(top) {
        top = p;
    }
    if top == tree.root() {
        return id;
    }
    survivors.get(&top).copied().unwrap_or_else(|| tree.root())
}

/// Everything about the scan loaded in one tab.
pub struct Session {
    pub epoch: u64,
    pub view: ViewOpts,
    pub scan: Scan,
    pub root: NodeId,
    pub selection: Option<NodeId>,
    /// A rescan of the view root is running.
    pub refreshing: bool,
    /// The file-system watcher of this tab, when watching.
    pub live: Option<Live>,
    /// The watcher lost changes, so the tree may be out of date.
    pub stale: bool,
    /// Changes of the last moments, for the highlight.
    pub marks: Marks,
    back: Vec<NodeId>,
    fwd: Vec<NodeId>,
    pub tags: Tags,
    pub tags_ver: u64,
    pub filter: FilterState,
    stats: Stats,
    restore: Option<Restore>,
}

impl Session {
    /// Creates a session around a started scan.
    pub fn new(epoch: u64, scan: Scan, view: ViewOpts) -> Self {
        let root = scan.tree().root();
        Session {
            epoch,
            view,
            scan,
            root,
            selection: None,
            refreshing: false,
            live: None,
            stale: false,
            marks: Marks::default(),
            back: Vec::new(),
            fwd: Vec::new(),
            tags: Tags::new(),
            tags_ver: 0,
            filter: FilterState::new(),
            stats: Stats::default(),
            restore: None,
        }
    }

    /// Notes the view root, selection and filter to bring back after a rescan.
    pub fn remember(&self) -> Restore {
        let tree = self.scan.tree();
        Restore {
            root: names_to(tree, self.root),
            selection: self.selection.map(|id| names_to(tree, id)),
            filter: self.filter.text.clone(),
        }
    }

    /// Applies the filter now and the view root and selection once the scan is done.
    pub fn restore_after_scan(&mut self, restore: Restore, classes: &FileClasses) {
        let _ = self.set_filter(&restore.filter, classes);
        self.restore = Some(restore);
    }

    /// Moves the view root and selection back to what a rescan replaced, when they still exist.
    pub fn finish_restore(&mut self) {
        let Some(r) = self.restore.take() else { return };
        let tree = self.scan.tree();
        let root = find_by_names(tree, &r.root).filter(|&n| tree.get(n).kind != NodeKind::File);
        let selection = r.selection.and_then(|names| find_by_names(tree, &names));
        if let Some(root) = root {
            self.root = root;
        }
        self.selection = selection;
    }

    /// Applies watcher events to the tree and keeps the view, history and filter valid.
    pub fn apply_live(
        &mut self,
        updater: &mut LiveUpdater,
        events: &[WatchEvent],
        classes: &FileClasses,
    ) {
        self.stale |= events.iter().any(|e| e.kind == WatchKind::Overflow);
        let root = self.scan.root_path.clone();
        let changes = updater.apply(self.scan.tree_mut(), Path::new(&root), events);
        if changes.is_empty() {
            return;
        }
        self.scan.touch();
        let tree = self.scan.tree();
        let survivors: HashMap<NodeId, NodeId> = changes
            .iter()
            .filter(|c| c.kind == ChangeKind::Removed)
            .map(|c| (c.node, nearest_existing(tree, &c.path)))
            .collect();
        let marked = changes.iter().map(|c| mark_of(c, &survivors));
        self.marks.add(marked, std::time::Instant::now());
        self.fix_refs(&survivors);
        self.refresh_filter(classes);
    }

    /// Path of a node relative to the scan root, separated by `\`.
    pub fn rel_path(&self, id: NodeId) -> String {
        names_to(self.scan.tree(), id).join("\\")
    }

    /// Drops a path removed by the app itself, like a watcher removal; a no-op once it is gone.
    pub fn forget_path(&mut self, rel: &str, classes: &FileClasses) {
        if self.scan.running {
            return;
        }
        let cluster = self
            .scan
            .volume
            .as_ref()
            .map_or(DEFAULT_CLUSTER, |v| v.cluster_size);
        let event = WatchEvent {
            path: rel.into(),
            kind: WatchKind::Removed,
        };
        self.apply_live(&mut LiveUpdater::new(cluster), &[event], classes);
    }

    /// Moves the view root, selection and history off removed nodes onto their nearest surviving ancestor.
    fn fix_refs(&mut self, survivors: &HashMap<NodeId, NodeId>) {
        let tree = self.scan.tree();
        let fix = |id: NodeId| survivor(tree, id, survivors);
        self.root = fix(self.root);
        self.selection = self.selection.map(fix);
        for id in self.back.iter_mut().chain(self.fwd.iter_mut()) {
            *id = fix(*id);
        }
    }

    /// Re-evaluates the filter and the statistics after the file classes changed.
    pub fn classes_changed(&mut self, classes: &FileClasses) {
        let text = std::mem::take(&mut self.filter.text);
        if self.set_filter(&text, classes).is_err() {
            self.filter.text = text;
        }
        self.filter.dirty = true;
        self.refresh_filter(classes);
        self.stats.key = None;
    }

    /// Sizes used for layout and statistics: filtered when a filter is active.
    pub fn sizes(&self) -> Sizes<'_> {
        self.filter
            .filtered
            .as_ref()
            .map_or(Sizes::Allocated, |f| Sizes::Slice(&f.sizes))
    }

    /// Size of a node under the active filter.
    pub fn size_of(&self, id: NodeId) -> u64 {
        self.sizes().get(self.scan.tree(), id)
    }

    /// Free and unknown space shown beside the contents of a volume root.
    pub fn free_unknown(&self) -> (u64, u64) {
        let tree = self.scan.tree();
        let Some(vol) = self.scan.volume.as_ref() else {
            return (0, 0);
        };
        if self.root != tree.root() || !vol.is_volume_root || self.filter.filtered.is_some() {
            return (0, 0);
        }
        let free = if self.view.show_free {
            vol.free_bytes
        } else {
            0
        };
        let used = vol.total_bytes.saturating_sub(vol.free_bytes);
        let unknown = if self.view.show_unknown {
            used.saturating_sub(tree.get(tree.root()).allocated)
        } else {
            0
        };
        (free, unknown)
    }

    // ----- navigation -----

    fn is_ancestor(&self, anc: NodeId, mut node: NodeId) -> bool {
        let tree = self.scan.tree();
        loop {
            if node == anc {
                return true;
            }
            match tree.parent(node) {
                Some(p) => node = p,
                None => return false,
            }
        }
    }

    /// Selects a node, or clears the selection. History is untouched.
    pub fn select(&mut self, node: Option<NodeId>) {
        self.selection = node;
    }

    /// Zooms to `node` (its parent for a file), recording the previous view root.
    pub fn zoom_to(&mut self, node: NodeId) {
        let tree = self.scan.tree();
        let target = if tree.get(node).kind == NodeKind::File {
            tree.parent(node).unwrap_or(node)
        } else {
            node
        };
        self.move_root(target, Some(target));
    }

    fn move_root(&mut self, target: NodeId, selection: Option<NodeId>) {
        if target == self.root {
            return;
        }
        self.back.push(self.root);
        self.fwd.clear();
        self.root = target;
        self.selection = selection;
    }

    /// Returns to the previous view root and selects the folder that was zoomed.
    pub fn go_back(&mut self) {
        let Some(prev) = self.back.pop() else { return };
        let old = self.root;
        self.fwd.push(old);
        self.root = prev;
        self.selection = Some(if self.is_ancestor(prev, old) {
            old
        } else {
            prev
        });
    }

    /// Redoes a zoom undone by [`Session::go_back`].
    pub fn go_forward(&mut self) {
        let Some(next) = self.fwd.pop() else { return };
        self.back.push(self.root);
        self.root = next;
        self.selection = Some(next);
    }

    /// Zooms out one level, selecting the folder left.
    pub fn go_up(&mut self) {
        if let Some(p) = self.scan.tree().parent(self.root) {
            self.move_root(p, Some(self.root));
        }
    }

    /// Zooms out to the scan root, selecting the folder left.
    pub fn go_home(&mut self) {
        let root = self.scan.tree().root();
        self.move_root(root, Some(self.root));
    }

    /// True when Back is available.
    pub fn can_back(&self) -> bool {
        !self.back.is_empty()
    }

    /// True when Forward is available.
    pub fn can_forward(&self) -> bool {
        !self.fwd.is_empty()
    }

    /// True when the view root can be rescanned on its own.
    pub fn can_rescan_view(&self) -> bool {
        self.can_up() && !self.scan.running && !self.refreshing
    }

    /// True when the view root has a parent.
    pub fn can_up(&self) -> bool {
        self.scan.tree().parent(self.root).is_some()
    }

    // ----- tags -----

    /// Toggles a tag on a node.
    pub fn toggle_tag(&mut self, node: NodeId, tag: Tag) {
        self.tags.toggle(node, tag);
        self.tags_ver += 1;
    }

    /// Clears tags under the view root.
    pub fn clear_tags(&mut self) {
        let root = self.root;
        self.tags.clear_subtree(self.scan.tree(), root);
        self.tags_ver += 1;
    }

    // ----- filter -----

    /// Number of files matching the filter.
    pub fn matched_files(&self) -> u64 {
        self.filter
            .filtered
            .as_ref()
            .map_or_else(|| self.scan.tree().file_count(), |f| f.matched_files)
    }

    /// Parses and applies filter text; on error the previous filter stays active.
    pub fn set_filter(&mut self, text: &str, classes: &FileClasses) -> Result<(), String> {
        if text == self.filter.text {
            return Ok(());
        }
        let parsed = Filter::parse(text, classes).map_err(|e| e.reason)?;
        self.filter.filter = parsed;
        self.filter.text = text.to_string();
        self.filter.dirty = true;
        self.refresh_filter(classes);
        Ok(())
    }

    /// Re-applies the filter when the tree or tags changed, throttled during a scan.
    pub fn refresh_filter(&mut self, classes: &FileClasses) {
        let fs = &mut self.filter;
        let tree_changed = fs.applied_tree_ver != self.scan.version;
        let tags_changed = fs.filter.uses_tags() && fs.applied_tags_ver != self.tags_ver;
        let stale = (tree_changed || tags_changed) && !fs.filter.is_void();
        let throttled = self.scan.running
            && !fs.dirty
            && fs.applied_at.is_some_and(|t| t.elapsed() < SCAN_REFRESH);
        if !(fs.dirty || stale) || throttled {
            return;
        }
        fs.dirty = false;
        fs.applied_tree_ver = self.scan.version;
        fs.applied_tags_ver = self.tags_ver;
        fs.applied_at = Some(Instant::now());
        fs.ver += 1;
        if fs.filter.is_void() {
            fs.filtered = None;
            fs.bold.clear();
            return;
        }
        let ctx = FilterCtx {
            tags: &self.tags,
            classes,
            now: fmt::filetime_now(),
        };
        let result = fs.filter.apply(self.scan.tree(), &ctx);
        fs.bold = result.bold_dirs.iter().copied().collect();
        fs.filtered = Some(result);
    }

    // ----- statistics -----

    /// Largest items and class totals under the view root, cached and throttled during a scan.
    pub fn stats(&mut self, classes: &FileClasses) -> &Stats {
        let key = (self.root, self.scan.version, self.filter.ver);
        if self.stats.key == Some(key) {
            return &self.stats;
        }
        let throttled = self.scan.running
            && self.stats.key.is_some_and(|k| k.0 == key.0)
            && self.stats.at.is_some_and(|t| t.elapsed() < SCAN_REFRESH);
        if !throttled {
            self.stats = self.compute_stats(classes, key);
        }
        &self.stats
    }

    fn compute_stats(&self, classes: &FileClasses, key: (NodeId, u64, u64)) -> Stats {
        let tree = self.scan.tree();
        let sizes = self.sizes();
        let mut files = BinaryHeap::new();
        let mut folders = BinaryHeap::new();
        let mut per_class = vec![0u64; classes.classes().len() + 1];
        let mut stack = vec![self.root];
        while let Some(id) = stack.pop() {
            let size = sizes.get(tree, id);
            if size == 0 {
                continue;
            }
            if tree.get(id).kind == NodeKind::File {
                push_top(&mut files, size, id);
                let c = classes
                    .class_of(tree.name(id))
                    .unwrap_or(per_class.len() - 1);
                per_class[c] += size;
            } else {
                if id != self.root {
                    push_top(&mut folders, size, id);
                }
                stack.extend(tree.children(id));
            }
        }
        Stats {
            files: into_sorted(files),
            folders: into_sorted(folders),
            classes: per_class,
            key: Some(key),
            at: Some(Instant::now()),
        }
    }
}

fn push_top(heap: &mut BinaryHeap<Reverse<(u64, NodeId)>>, size: u64, id: NodeId) {
    if heap.len() < TOP_N {
        heap.push(Reverse((size, id)));
    } else if heap.peek().is_some_and(|Reverse((min, _))| size > *min) {
        heap.pop();
        heap.push(Reverse((size, id)));
    }
}

fn into_sorted(heap: BinaryHeap<Reverse<(u64, NodeId)>>) -> Vec<(NodeId, u64)> {
    let mut v: Vec<_> = heap.into_iter().map(|Reverse((s, id))| (id, s)).collect();
    v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::BackendPref;
    use std::fs;

    fn scanned(root: &std::path::Path) -> Session {
        let (mut scan, rx) = Scan::start(root, None, BackendPref::Walk);
        while !scan.apply(rx.recv().expect("scan events")) {}
        Session::new(1, scan, ViewOpts::from_config(&Config::default()))
    }

    fn event(path: &str, kind: WatchKind) -> WatchEvent {
        WatchEvent {
            path: path.into(),
            kind,
        }
    }

    #[test]
    fn live_events_match_a_fresh_scan_and_keep_the_view_valid() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        fs::create_dir_all(root.join("a/b")).expect("mkdir");
        fs::write(root.join("a/b/f.bin"), vec![0u8; 5000]).expect("write");
        fs::write(root.join("a/g.bin"), vec![0u8; 7000]).expect("write");
        fs::write(root.join("h.bin"), vec![0u8; 100]).expect("write");
        let mut live = scanned(root);
        let (b, f) = (
            find_by_names(live.scan.tree(), &["a", "b"]),
            find_by_names(live.scan.tree(), &["a", "b", "f.bin"]),
        );
        live.zoom_to(b.expect("b"));
        live.select(f);

        fs::remove_dir_all(root.join("a/b")).expect("remove");
        fs::write(root.join("a/new.bin"), vec![0u8; 9000]).expect("write");
        fs::write(root.join("h.bin"), vec![0u8; 20_000]).expect("grow");
        let events = [
            event("a\\b", WatchKind::Removed),
            event("a\\b\\f.bin", WatchKind::Removed),
            event("a\\new.bin", WatchKind::Added),
            event("h.bin", WatchKind::Modified),
        ];
        let cluster = diskmap_scan::volume::volume_info(root)
            .expect("volume")
            .cluster_size;
        live.apply_live(
            &mut LiveUpdater::new(cluster),
            &events,
            &FileClasses::default(),
        );

        let fresh = scanned(root);
        let (lt, ft) = (live.scan.tree(), fresh.scan.tree());
        assert_eq!(lt.get(lt.root()).allocated, ft.get(ft.root()).allocated);
        assert_eq!(
            (lt.file_count(), lt.dir_count()),
            (ft.file_count(), ft.dir_count())
        );
        let a = find_by_names(lt, &["a"]).expect("a");
        assert_eq!((live.root, live.selection), (a, Some(a)));
        assert!(live.can_back() && !live.stale);
    }

    #[test]
    fn restore_brings_back_view_root_and_selection() {
        let dir = tempfile::tempdir().expect("tempdir");
        fs::create_dir_all(dir.path().join("a/b")).expect("mkdir");
        fs::write(dir.path().join("a/b/f.bin"), vec![0u8; 5000]).expect("write");
        let mut old = scanned(dir.path());
        let tree = old.scan.tree();
        let folder = find_by_names(tree, &["a"]).expect("folder a");
        let file = find_by_names(tree, &["a", "b", "f.bin"]).expect("file");
        old.zoom_to(folder);
        old.select(Some(file));
        let mut fresh = scanned(dir.path());
        fresh.restore_after_scan(old.remember(), &FileClasses::default());
        fresh.finish_restore();
        assert_eq!(
            fresh.root,
            find_by_names(fresh.scan.tree(), &["a"]).expect("a")
        );
        assert_eq!(
            fresh.selection,
            find_by_names(fresh.scan.tree(), &["a", "b", "f.bin"])
        );
    }
}
