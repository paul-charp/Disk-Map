//! Backend-neutral scan events and the scanner trait.

use crossbeam_channel::Sender;
use std::ops::Range;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

/// Scanner-assigned id of a directory; the scan root is token 0.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DirToken(pub u32);

impl DirToken {
    /// Token of the scan root.
    pub const ROOT: DirToken = DirToken(0);
}

/// One directory entry. Timestamps are Windows FILETIME ticks.
#[derive(Clone, Debug)]
pub struct Entry {
    /// Byte range of the name inside [`DirBatch::names`].
    pub name: Range<u32>,
    pub logical: u64,
    pub allocated: u64,
    pub ctime: i64,
    pub mtime: i64,
    pub atime: i64,
    /// Raw Windows file attributes.
    pub attrs: u32,
    /// Token of the sub-directory, which gets its own later batch.
    pub dir: Option<DirToken>,
}

/// A slice of one directory listing.
#[derive(Clone, Debug)]
pub struct DirBatch {
    pub parent: DirToken,
    /// All entry names concatenated.
    pub names: String,
    pub entries: Vec<Entry>,
}

impl DirBatch {
    /// Name of an entry.
    pub fn name(&self, entry: &Entry) -> &str {
        &self.names[entry.name.start as usize..entry.name.end as usize]
    }
}

/// Message from a scanner to the owner of the tree.
#[derive(Debug)]
pub enum ScanEvent {
    Batch(DirBatch),
    /// A directory could not be opened or listed; the scan continues.
    Error {
        path: String,
        code: u32,
    },
    /// Sent exactly once, after every other event.
    Done {
        cancelled: bool,
    },
}

/// A scan backend.
pub trait Scanner: Send {
    /// Starts scanning without blocking; events arrive on `tx`.
    fn start(self: Box<Self>, root: PathBuf, tx: Sender<ScanEvent>, cancel: Arc<AtomicBool>);
}
