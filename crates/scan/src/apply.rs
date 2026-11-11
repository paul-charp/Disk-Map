//! Applies scan events to a [`Tree`].

use crate::event::{DirBatch, DirToken, ScanEvent};
use crossbeam_channel::Receiver;
use diskmap_core::tree::{NewEntry, NodeId, NodeKind, Times, Tree};

const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x10;
const UNMAPPED: u32 = u32::MAX;

/// A scan error reported by a backend.
#[derive(Clone, Debug)]
pub struct ScanError {
    pub path: String,
    pub code: u32,
}

/// Outcome of [`TreeBuilder::drain`].
#[derive(Debug, Default)]
pub struct DrainResult {
    pub errors: Vec<ScanError>,
    pub cancelled: bool,
}

/// Owns a [`Tree`] and applies [`DirBatch`]es to it.
pub struct TreeBuilder {
    tree: Tree,
    nodes: Vec<u32>,
}

impl TreeBuilder {
    /// Creates a builder whose root node is named `root_path`.
    pub fn new(root_path: &str) -> Self {
        TreeBuilder {
            tree: Tree::new(root_path),
            nodes: vec![0],
        }
    }

    /// The tree built so far.
    pub fn tree(&self) -> &Tree {
        &self.tree
    }

    /// Mutable tree, for replacing a subtree once the scan is done.
    pub fn tree_mut(&mut self) -> &mut Tree {
        &mut self.tree
    }

    /// Consumes the builder, returning the tree.
    pub fn into_tree(self) -> Tree {
        self.tree
    }

    fn node_of(&self, token: DirToken) -> Option<NodeId> {
        match self.nodes.get(token.0 as usize) {
            Some(&n) if n != UNMAPPED => Some(NodeId(n)),
            _ => None,
        }
    }

    /// Applies one batch. Batches whose parent is unknown are dropped.
    pub fn apply_batch(&mut self, batch: &DirBatch) {
        let Some(parent) = self.node_of(batch.parent) else {
            return;
        };
        for entry in &batch.entries {
            let is_dir = entry.dir.is_some() || entry.attrs & FILE_ATTRIBUTE_DIRECTORY != 0;
            let id = self.tree.add_child(
                parent,
                NewEntry {
                    name: batch.name(entry),
                    kind: if is_dir {
                        NodeKind::Dir
                    } else {
                        NodeKind::File
                    },
                    attrs: entry.attrs,
                    logical: entry.logical,
                    allocated: entry.allocated,
                    times: Times {
                        ctime: entry.ctime,
                        mtime: entry.mtime,
                        atime: entry.atime,
                    },
                },
            );
            if let Some(token) = entry.dir {
                let slot = token.0 as usize;
                if self.nodes.len() <= slot {
                    self.nodes.resize(slot + 1, UNMAPPED);
                }
                self.nodes[slot] = id.0;
            }
        }
    }

    /// Applies events until `Done` (or the channel closes).
    pub fn drain(&mut self, rx: &Receiver<ScanEvent>) -> DrainResult {
        let mut result = DrainResult::default();
        for event in rx {
            match event {
                ScanEvent::Batch(batch) => self.apply_batch(&batch),
                ScanEvent::Error { path, code } => result.errors.push(ScanError { path, code }),
                ScanEvent::Done { cancelled } => {
                    result.cancelled = cancelled;
                    break;
                }
            }
        }
        result
    }
}
