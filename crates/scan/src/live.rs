//! Applies watcher events to a [`Tree`].

use crate::apply::TreeBuilder;
use crate::event::Scanner;
use crate::walk::{WalkScanner, absolute_root, extended_path};
use crate::watch::{WatchEvent, WatchKind};
use diskmap_core::tree::{NewEntry, NodeId, NodeKind, Times, Tree};
use std::collections::{HashMap, HashSet};
use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use windows::Win32::Foundation::{
    ERROR_FILE_NOT_FOUND, ERROR_INVALID_NAME, ERROR_PATH_NOT_FOUND, GetLastError, NO_ERROR,
};
use windows::Win32::Storage::FileSystem::{
    FILE_ATTRIBUTE_COMPRESSED, FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT,
    FILE_ATTRIBUTE_SPARSE_FILE, GetCompressedFileSizeW, GetFileAttributesExW,
    GetFileExInfoStandard, INVALID_FILE_SIZE, WIN32_FILE_ATTRIBUTE_DATA,
};
use windows::core::PCWSTR;

/// Directories with at least this many children get a hashed name index during a burst.
const INDEX_THRESHOLD: u32 = 64;

/// What changed for a node.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChangeKind {
    Added,
    Removed,
    Grew,
    Shrank,
}

/// One effect of [`LiveUpdater::apply`] on the tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Change {
    /// For `Removed` the node is detached from the tree but keeps its last sizes and name.
    pub node: NodeId,
    pub kind: ChangeKind,
    /// Path relative to the scan root, separated by `\`.
    pub path: String,
}

/// Turns raw watcher events into tree mutations.
pub struct LiveUpdater {
    cluster: u64,
}

struct Stat {
    attrs: u32,
    is_dir: bool,
    reparse: bool,
    logical: u64,
    allocated: u64,
    times: Times,
}

enum StatError {
    Gone,
    Other,
}

fn ticks(low: u32, high: u32) -> i64 {
    (u64::from(high) << 32 | u64::from(low)) as i64
}

fn stat(full: &Path, cluster: u64) -> Result<Stat, StatError> {
    let mut w = extended_path(full);
    w.push(0);
    let mut data = WIN32_FILE_ATTRIBUTE_DATA::default();
    // SAFETY: w is NUL-terminated and data is a live WIN32_FILE_ATTRIBUTE_DATA.
    let r = unsafe {
        GetFileAttributesExW(
            PCWSTR(w.as_ptr()),
            GetFileExInfoStandard,
            (&mut data as *mut WIN32_FILE_ATTRIBUTE_DATA).cast::<c_void>(),
        )
    };
    if let Err(e) = r {
        let gone = [
            ERROR_FILE_NOT_FOUND,
            ERROR_PATH_NOT_FOUND,
            ERROR_INVALID_NAME,
        ]
        .iter()
        .any(|c| e.code() == c.to_hresult());
        return Err(if gone {
            StatError::Gone
        } else {
            StatError::Other
        });
    }
    let attrs = data.dwFileAttributes;
    let is_dir = attrs & FILE_ATTRIBUTE_DIRECTORY.0 != 0;
    let reparse = attrs & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0;
    let (mut logical, mut allocated) = (0, 0);
    if !is_dir && !reparse {
        logical = u64::from(data.nFileSizeHigh) << 32 | u64::from(data.nFileSizeLow);
        allocated = logical.next_multiple_of(cluster.max(1));
        if attrs & (FILE_ATTRIBUTE_COMPRESSED.0 | FILE_ATTRIBUTE_SPARSE_FILE.0) != 0 {
            let mut high = 0u32;
            // SAFETY: w is NUL-terminated and high is a live out parameter.
            let low = unsafe { GetCompressedFileSizeW(PCWSTR(w.as_ptr()), Some(&mut high)) };
            // SAFETY: reads the calling thread's last error.
            if low != INVALID_FILE_SIZE || unsafe { GetLastError() } == NO_ERROR {
                allocated = u64::from(high) << 32 | u64::from(low);
            }
        }
    }
    Ok(Stat {
        attrs,
        is_dir,
        reparse,
        logical,
        allocated,
        times: Times {
            ctime: ticks(
                data.ftCreationTime.dwLowDateTime,
                data.ftCreationTime.dwHighDateTime,
            ),
            mtime: ticks(
                data.ftLastWriteTime.dwLowDateTime,
                data.ftLastWriteTime.dwHighDateTime,
            ),
            atime: ticks(
                data.ftLastAccessTime.dwLowDateTime,
                data.ftLastAccessTime.dwHighDateTime,
            ),
        },
    })
}

fn key(name: &str) -> String {
    name.to_lowercase()
}

/// Name lookup that switches to a hashed per-directory index for big directories.
#[derive(Default)]
struct Lookup {
    dirs: HashMap<NodeId, HashMap<String, NodeId>>,
}

impl Lookup {
    fn child(&mut self, tree: &Tree, dir: NodeId, name: &str) -> Option<NodeId> {
        if tree.get(dir).child_count() < INDEX_THRESHOLD && !self.dirs.contains_key(&dir) {
            return tree.find_child(dir, name);
        }
        let map = self
            .dirs
            .entry(dir)
            .or_insert_with(|| tree.children(dir).map(|c| (key(tree.name(c)), c)).collect());
        map.get(&key(name)).copied()
    }

    fn path(&mut self, tree: &Tree, rel: &str) -> Option<NodeId> {
        let mut cur = tree.root();
        for part in rel.split('\\').filter(|p| !p.is_empty()) {
            cur = self.child(tree, cur, part)?;
        }
        Some(cur)
    }

    fn added(&mut self, dir: NodeId, name: &str, id: NodeId) {
        if let Some(m) = self.dirs.get_mut(&dir) {
            m.insert(key(name), id);
        }
    }

    fn removed(&mut self, dir: NodeId, name: &str) {
        if let Some(m) = self.dirs.get_mut(&dir) {
            m.remove(&key(name));
        }
    }
}

fn split(rel: &str) -> (&str, &str) {
    rel.rsplit_once('\\').unwrap_or(("", rel))
}

impl LiveUpdater {
    /// Creates an updater; `cluster_size` rounds allocated sizes of plain files.
    pub fn new(cluster_size: u64) -> Self {
        LiveUpdater {
            cluster: cluster_size,
        }
    }

    /// Re-stats every distinct path of `events` once and brings `tree` in line with the disk.
    /// `root` is the directory the tree was scanned from.
    pub fn apply(&mut self, tree: &mut Tree, root: &Path, events: &[WatchEvent]) -> Vec<Change> {
        let mut seen = HashSet::new();
        let mut paths: Vec<&str> = Vec::new();
        for e in events {
            let (from, to) = match &e.kind {
                WatchKind::Overflow => continue,
                WatchKind::Renamed { from } => (Some(from.as_str()), e.path.as_str()),
                _ => (None, e.path.as_str()),
            };
            for p in from.into_iter().chain([to]) {
                let p = p.trim_matches('\\');
                if !p.is_empty() && seen.insert(key(p)) {
                    paths.push(p);
                }
            }
        }
        paths.sort_by_cached_key(|p| p.matches('\\').count());

        let base = absolute_root(root);
        let base = base.trim_end_matches('\\');
        let mut lookup = Lookup::default();
        let mut changes = Vec::new();
        for rel in paths {
            let full = PathBuf::from(format!("{base}\\{rel}"));
            self.update_path(tree, &mut lookup, rel, &full, &mut changes);
        }
        changes
    }

    fn update_path(
        &self,
        tree: &mut Tree,
        lookup: &mut Lookup,
        rel: &str,
        full: &Path,
        out: &mut Vec<Change>,
    ) {
        let (parent_rel, name) = split(rel);
        let st = match stat(full, self.cluster) {
            Ok(st) => st,
            Err(StatError::Gone) => {
                if let Some(id) = lookup.path(tree, rel) {
                    let parent = tree.parent(id);
                    if tree.remove(id) {
                        if let Some(p) = parent {
                            lookup.removed(p, name);
                        }
                        out.push(Change {
                            node: id,
                            kind: ChangeKind::Removed,
                            path: rel.to_string(),
                        });
                    }
                }
                return;
            }
            Err(StatError::Other) => return,
        };
        if let Some(id) = lookup.path(tree, rel) {
            let node = tree.get(id);
            if (node.kind == NodeKind::File) != st.is_dir {
                let (old_logical, old_allocated) = (node.logical, node.allocated);
                let is_file = node.kind == NodeKind::File;
                tree.set_file(id, st.logical, st.allocated, st.times, st.attrs);
                let kind = if !is_file {
                    None
                } else {
                    match (
                        st.logical.cmp(&old_logical),
                        st.allocated.cmp(&old_allocated),
                    ) {
                        (std::cmp::Ordering::Greater, _)
                        | (std::cmp::Ordering::Equal, std::cmp::Ordering::Greater) => {
                            Some(ChangeKind::Grew)
                        }
                        (std::cmp::Ordering::Less, _)
                        | (std::cmp::Ordering::Equal, std::cmp::Ordering::Less) => {
                            Some(ChangeKind::Shrank)
                        }
                        _ => None,
                    }
                };
                if let Some(kind) = kind {
                    out.push(Change {
                        node: id,
                        kind,
                        path: rel.to_string(),
                    });
                }
                return;
            }
            // a file became a directory or the other way round
            let parent = tree.parent(id);
            if tree.remove(id) {
                if let Some(p) = parent {
                    lookup.removed(p, name);
                }
                out.push(Change {
                    node: id,
                    kind: ChangeKind::Removed,
                    path: rel.to_string(),
                });
            }
        }
        let Some(parent) = lookup
            .path(tree, parent_rel)
            .filter(|&p| tree.get(p).kind != NodeKind::File)
        else {
            return;
        };
        let id = tree.add_child(
            parent,
            NewEntry {
                name,
                kind: if st.is_dir {
                    NodeKind::Dir
                } else {
                    NodeKind::File
                },
                attrs: st.attrs,
                logical: st.logical,
                allocated: st.allocated,
                times: st.times,
            },
        );
        lookup.added(parent, name, id);
        if st.is_dir && !st.reparse {
            walk_into(tree, id, full);
        }
        out.push(Change {
            node: id,
            kind: ChangeKind::Added,
            path: rel.to_string(),
        });
    }
}

/// Scans `dir` with the walk backend and grafts its content under `id`.
fn walk_into(tree: &mut Tree, id: NodeId, dir: &Path) {
    let (tx, rx) = crossbeam_channel::unbounded();
    let mut builder = TreeBuilder::new(&absolute_root(dir));
    Box::new(WalkScanner::new(Some(2))).start(
        dir.to_path_buf(),
        tx,
        Arc::new(AtomicBool::new(false)),
    );
    builder.drain(&rx);
    let sub = builder.tree();
    tree.graft_children(id, sub, sub.root());
}
