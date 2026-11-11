//! Backend-neutral disk scanning: events, a parallel directory walker and a tree builder.

pub mod apply;
pub mod event;
pub mod live;
pub mod mft;
pub mod volume;
pub mod walk;
pub mod watch;

pub use apply::{DrainResult, ScanError, TreeBuilder};
pub use event::{DirBatch, DirToken, Entry, ScanEvent, Scanner};
pub use live::{Change, ChangeKind, LiveUpdater};
pub use mft::{
    Backend, MftScanner, MftStats, MftSupport, auto_scanner, is_elevated, mft_available,
};
pub use walk::WalkScanner;
pub use watch::{WatchEvent, WatchKind, Watcher};
