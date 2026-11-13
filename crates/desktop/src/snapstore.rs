//! Full snapshots on disk: `<data>/snapshots/<root key>/<YYYYMMDD-HHMMSS>[-keep].dmsn` (UTC).

use crate::paths;
use diskmap_core::export::iso_utc;
use diskmap_core::key::root_key;
use diskmap_core::snapshot::{self, SnapshotInfo, SnapshotMeta};
use diskmap_core::tree::Tree;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

const DIR: &str = "snapshots";
const EXT: &str = "dmsn";
const KEEP: &str = "-keep";
/// Bytes read for the header; enough for any root path.
const HEADER_BYTES: u64 = 64 * 1024;

/// One stored snapshot.
#[derive(Clone, Debug)]
pub struct Entry {
    pub path: PathBuf,
    /// Size of the file.
    pub bytes: u64,
    /// Kept by hand: never pruned.
    pub kept: bool,
    pub info: SnapshotInfo,
}

/// Folder of the snapshots of a scan root; created when missing.
fn root_dir(root: &str) -> io::Result<PathBuf> {
    let dir = paths::ensure_data_dir()?.join(DIR).join(root_key(root));
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// `YYYYMMDD-HHMMSS` of a FILETIME, in UTC.
fn stamp(filetime: i64) -> String {
    iso_utc(filetime)
        .chars()
        .filter(char::is_ascii_digit)
        .enumerate()
        .flat_map(|(i, c)| (i == 8).then_some('-').into_iter().chain([c]))
        .collect()
}

/// A free file name in `dir` for a snapshot taken at `time`.
fn file_name(dir: &Path, time: i64, kept: bool) -> PathBuf {
    let base = format!("{}{}", stamp(time), if kept { KEEP } else { "" });
    let mut path = dir.join(format!("{base}.{EXT}"));
    let mut n = 1;
    while path.exists() {
        n += 1;
        path = dir.join(format!("{base}-{n}.{EXT}"));
    }
    path
}

/// Writes a snapshot of `tree` next to the others of its root; returns its path.
pub fn save(tree: &Tree, meta: &SnapshotMeta, kept: bool) -> io::Result<PathBuf> {
    save_in(&root_dir(tree.name(tree.root()))?, tree, meta, kept)
}

fn save_in(dir: &Path, tree: &Tree, meta: &SnapshotMeta, kept: bool) -> io::Result<PathBuf> {
    let bytes = snapshot::encode(tree, meta);
    let path = file_name(dir, meta.scan_time, kept);
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, bytes)?;
    fs::rename(&tmp, &path).inspect_err(|_| {
        let _ = fs::remove_file(&tmp);
    })?;
    Ok(path)
}

/// Snapshots of a root, newest first.
pub fn list(root: &str) -> Vec<Entry> {
    root_dir(root).map(|d| list_in(&d)).unwrap_or_default()
}

fn list_in(dir: &Path) -> Vec<Entry> {
    let Ok(read) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<Entry> = read
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == EXT))
        .filter_map(|p| entry(&p))
        .collect();
    out.sort_by(|a, b| {
        b.info
            .meta
            .scan_time
            .cmp(&a.info.meta.scan_time)
            .then_with(|| b.path.cmp(&a.path))
    });
    out
}

fn entry(path: &Path) -> Option<Entry> {
    let file = fs::File::open(path).ok()?;
    let bytes = file.metadata().ok()?.len();
    let mut head = Vec::new();
    file.take(HEADER_BYTES).read_to_end(&mut head).ok()?;
    let info = snapshot::read_info(&head).ok()?;
    let stem = path.file_stem()?.to_string_lossy();
    Some(Entry {
        path: path.to_path_buf(),
        bytes,
        kept: stem.contains(KEEP),
        info,
    })
}

/// Reads and decodes a snapshot.
pub fn load(path: &Path) -> Result<(Tree, SnapshotMeta), String> {
    let bytes = fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    snapshot::decode(&bytes).map_err(|e| format!("{}: {e}", path.display()))
}

/// True when `path` is a snapshot file inside the snapshot folder.
pub fn is_snapshot(path: &Path) -> bool {
    let Ok(data) = paths::ensure_data_dir() else {
        return false;
    };
    let base = data.join(DIR);
    path.extension().is_some_and(|e| e == EXT)
        && path
            .parent()
            .and_then(Path::parent)
            .is_some_and(|p| p == base)
}

/// Deletes one snapshot file.
pub fn delete(path: &Path) -> Result<(), String> {
    if !is_snapshot(path) {
        return Err(format!("Not a snapshot: {}", path.display()));
    }
    fs::remove_file(path).map_err(|e| format!("{}: {e}", path.display()))
}

/// Deletes the oldest snapshots of a root beyond the newest `keep`, sparing kept ones; returns how many went.
pub fn prune(root: &str, keep: usize) -> usize {
    root_dir(root).map(|d| prune_in(&d, keep)).unwrap_or(0)
}

fn prune_in(dir: &Path, keep: usize) -> usize {
    list_in(dir)
        .into_iter()
        .filter(|e| !e.kept)
        .skip(keep)
        .filter(|e| fs::remove_file(&e.path).is_ok())
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use diskmap_core::tree::{NewEntry, NodeKind, Times};

    fn tree() -> Tree {
        let mut t = Tree::new("C:\\data");
        let r = t.root();
        t.add_child(
            r,
            NewEntry {
                name: "a.bin",
                kind: NodeKind::File,
                attrs: 0x20,
                logical: 5000,
                allocated: 8192,
                times: Times::default(),
            },
        );
        t
    }

    fn meta(scan_time: i64) -> SnapshotMeta {
        SnapshotMeta {
            scan_time,
            ..SnapshotMeta::default()
        }
    }

    #[test]
    fn names_are_utc_stamps() {
        assert_eq!(stamp(133_000_000_000_000_000), "20220618-042640");
    }

    #[test]
    fn save_list_and_prune_spare_kept() {
        let dir = tempfile::tempdir().expect("tempdir");
        let t = tree();
        let day = 864_000_000_000;
        let base = 133_000_000_000_000_000;
        let kept = save_in(dir.path(), &t, &meta(base), true).expect("save");
        for i in 1..=4 {
            save_in(dir.path(), &t, &meta(base + i * day), false).expect("save");
        }
        let list = list_in(dir.path());
        assert_eq!(list.len(), 5);
        assert_eq!(list[0].info.meta.scan_time, base + 4 * day);
        assert_eq!(list[0].info.allocated, 8192);
        assert_eq!(prune_in(dir.path(), 2), 2);
        let left = list_in(dir.path());
        assert_eq!(left.len(), 3);
        assert!(left.iter().any(|e| e.path == kept && e.kept));
        let (back, _) = load(&kept).expect("load");
        assert_eq!(back.file_count(), 1);
    }
}
