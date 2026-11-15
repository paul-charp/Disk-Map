//! Timeline point: the large or shallow folders of a [`Tree`] with their totals.

use crate::codec::{Reader, put_header, put_i64, put_str, put_u64};
use crate::tree::{NodeId, NodeKind, Tree};

pub use crate::codec::Error;

const MAGIC: &[u8; 4] = b"DMTP";
const VERSION: u16 = 1;
const PPM: f64 = 1_000_000.0;

/// Which folders a timeline point keeps.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SummaryOptions {
    /// Keep folders whose allocated size is at least this fraction of the root's.
    pub min_fraction: f64,
    /// Keep every folder at this depth or less (top-level folders are depth 1).
    pub max_depth: u16,
}

impl Default for SummaryOptions {
    fn default() -> Self {
        SummaryOptions {
            min_fraction: 0.001,
            max_depth: 2,
        }
    }
}

/// One folder of a timeline point. Counts and sizes are subtree totals.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SummaryEntry {
    /// Path relative to the root, components joined with `\`; empty for the root.
    pub path: String,
    /// Index of the parent entry; `None` for the root.
    pub parent: Option<u32>,
    /// Depth below the root (root 0).
    pub depth: u16,
    /// Allocated bytes.
    pub allocated: u64,
    /// Logical bytes.
    pub logical: u64,
    /// Files below.
    pub files: u64,
    /// Folders below.
    pub dirs: u64,
}

impl SummaryEntry {
    /// Last path component.
    pub fn name(&self) -> &str {
        self.path.rsplit('\\').next().unwrap_or("")
    }
}

/// A timeline point. `entries[0]` is the root with the totals; entries are in pre-order.
#[derive(Clone, Debug, PartialEq)]
pub struct Summary {
    /// Scan root path.
    pub root_path: String,
    /// Scan time (FILETIME ticks).
    pub scan_time: i64,
    /// Options the point was made with (`min_fraction` rounded to millionths).
    pub options: SummaryOptions,
    /// Root first, then kept folders in pre-order.
    pub entries: Vec<SummaryEntry>,
}

impl Summary {
    /// Builds a timeline point from a tree.
    pub fn from_tree(tree: &Tree, scan_time: i64, options: SummaryOptions) -> Self {
        let options = SummaryOptions {
            min_fraction: (options.min_fraction.clamp(0.0, 1.0) * PPM).round() / PPM,
            ..options
        };
        let counts = subtree_counts(tree);
        let root = tree.root();
        let min = (tree.get(root).allocated as f64 * options.min_fraction).ceil() as u64;
        let entry = |id: NodeId, path: String, parent: Option<u32>, depth: u16| {
            let n = tree.get(id);
            let (files, dirs) = counts[id.0 as usize];
            SummaryEntry {
                path,
                parent,
                depth,
                allocated: n.allocated,
                logical: n.logical,
                files,
                dirs,
            }
        };
        let mut entries = Vec::new();
        let mut stack = vec![(root, None, 0u16, String::new())];
        while let Some((id, parent, depth, path)) = stack.pop() {
            let index = entries.len() as u32;
            for c in tree.children(id) {
                let n = tree.get(c);
                if n.kind != NodeKind::Dir || (depth >= options.max_depth && n.allocated < min) {
                    continue;
                }
                let name = tree.name(c);
                let sub = if path.is_empty() {
                    name.to_owned()
                } else {
                    format!("{path}\\{name}")
                };
                stack.push((c, Some(index), depth + 1, sub));
            }
            entries.push(entry(id, path, parent, depth));
        }
        Summary {
            root_path: tree.name(root).to_owned(),
            scan_time,
            options,
            entries,
        }
    }

    /// Root totals.
    pub fn root(&self) -> &SummaryEntry {
        &self.entries[0]
    }

    /// Encodes the point.
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(32 + self.entries.len() * 24);
        put_header(&mut out, MAGIC, VERSION);
        put_i64(&mut out, self.scan_time);
        put_u64(&mut out, (self.options.min_fraction * PPM).round() as u64);
        put_u64(&mut out, u64::from(self.options.max_depth));
        put_str(&mut out, &self.root_path);
        put_u64(&mut out, self.entries.len() as u64);
        for (i, e) in self.entries.iter().enumerate() {
            if let Some(p) = e.parent {
                put_u64(&mut out, i as u64 - u64::from(p));
                put_str(&mut out, e.name());
            }
            put_u64(&mut out, e.allocated);
            put_u64(&mut out, e.logical);
            put_u64(&mut out, e.files);
            put_u64(&mut out, e.dirs);
        }
        out
    }

    /// Reads the scan time and root totals only: the header and the first entry, so a prefix of the file is enough.
    pub fn read_root(buf: &[u8]) -> Result<(i64, SummaryEntry), Error> {
        let mut r = Reader::open(buf, MAGIC, VERSION)?;
        let scan_time = r.i64()?;
        r.u64()?;
        r.u64()?;
        r.str()?;
        if r.u64()? == 0 {
            return Err(Error::Invalid("no root entry"));
        }
        let root = SummaryEntry {
            path: String::new(),
            parent: None,
            depth: 0,
            allocated: r.u64()?,
            logical: r.u64()?,
            files: r.u64()?,
            dirs: r.u64()?,
        };
        Ok((scan_time, root))
    }

    /// Decodes a point written by [`Summary::encode`].
    pub fn decode(buf: &[u8]) -> Result<Self, Error> {
        let mut r = Reader::open(buf, MAGIC, VERSION)?;
        let scan_time = r.i64()?;
        let ppm = r.u64()?;
        let max_depth = u16::try_from(r.u64()?).map_err(|_| Error::Invalid("depth"))?;
        let root_path = r.str()?.to_owned();
        let count = r.u64()?;
        if count == 0 {
            return Err(Error::Invalid("no root entry"));
        }
        let mut entries: Vec<SummaryEntry> = Vec::with_capacity(count.min(1 << 20) as usize);
        for i in 0..count {
            let (path, parent, depth) = if i == 0 {
                (String::new(), None, 0)
            } else {
                let back = r.u64()?;
                let p = i
                    .checked_sub(back)
                    .filter(|_| back > 0)
                    .ok_or(Error::Invalid("bad parent"))?;
                let parent = &entries[p as usize];
                let name = r.str()?;
                let path = if parent.path.is_empty() {
                    name.to_owned()
                } else {
                    format!("{}\\{name}", parent.path)
                };
                (path, Some(p as u32), parent.depth.saturating_add(1))
            };
            entries.push(SummaryEntry {
                path,
                parent,
                depth,
                allocated: r.u64()?,
                logical: r.u64()?,
                files: r.u64()?,
                dirs: r.u64()?,
            });
        }
        if !r.is_at_end() {
            return Err(Error::Invalid("trailing bytes"));
        }
        Ok(Summary {
            root_path,
            scan_time,
            options: SummaryOptions {
                min_fraction: ppm as f64 / PPM,
                max_depth,
            },
            entries,
        })
    }
}

/// File and folder counts below every node, indexed by node id.
fn subtree_counts(tree: &Tree) -> Vec<(u64, u64)> {
    let mut counts = vec![(0u64, 0u64); tree.len()];
    for i in (1..tree.len()).rev() {
        let id = NodeId(i as u32);
        let Some(parent) = tree.parent(id) else {
            continue;
        };
        let (f, d) = counts[i];
        let own = match tree.get(id).kind {
            NodeKind::File => (1, 0),
            _ => (0, 1),
        };
        let p = &mut counts[parent.0 as usize];
        p.0 += f + own.0;
        p.1 += d + own.1;
    }
    counts
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snapshot::tests::{dir, file};

    fn tree() -> Tree {
        let mut t = Tree::new("D:\\");
        let r = t.root();
        let big = t.add_child(r, dir("big"));
        let deep = t.add_child(big, dir("a"));
        let deeper = t.add_child(deep, dir("b"));
        let deepest = t.add_child(deeper, dir("c"));
        t.add_child(deepest, file("huge.bin", 1 << 30, 0));
        let tiny = t.add_child(deeper, dir("tiny"));
        t.add_child(tiny, file("t", 10, 0));
        let small = t.add_child(r, dir("small"));
        let s1 = t.add_child(small, dir("s1"));
        let s2 = t.add_child(s1, dir("s2"));
        t.add_child(s2, file("f", 100, 0));
        t.add_child(r, file("loose", 5, 0));
        t
    }

    fn paths(s: &Summary) -> Vec<&str> {
        s.entries.iter().map(|e| e.path.as_str()).collect()
    }

    #[test]
    fn reads_root_from_a_prefix() {
        let s = Summary::from_tree(&tree(), 42, SummaryOptions::default());
        let bytes = s.encode();
        let prefix = 16 + s.root_path.len() + 4 * 10;
        let (time, root) = Summary::read_root(&bytes[..prefix.min(bytes.len())]).unwrap();
        assert_eq!(time, 42);
        assert_eq!(&root, s.root());
    }

    #[test]
    fn keeps_large_or_shallow_folders() {
        let s = Summary::from_tree(&tree(), 42, SummaryOptions::default());
        assert_eq!(
            paths(&s),
            [
                "",
                "big",
                "big\\a",
                "big\\a\\b",
                "big\\a\\b\\c",
                "small",
                "small\\s1"
            ]
        );
        let root = s.root();
        assert_eq!((root.files, root.dirs), (4, 8));
        assert_eq!(root.allocated, (1 << 30) + 3 * 4096);
        let b = &s.entries[3];
        assert_eq!((b.files, b.dirs, b.depth), (2, 2, 3));
        assert_eq!(s.entries[b.parent.unwrap() as usize].path, "big\\a");
        assert_eq!(s.entries[4].name(), "c");
    }

    #[test]
    fn round_trip() {
        let opts = SummaryOptions {
            min_fraction: 0.0,
            max_depth: 0,
        };
        for s in [
            Summary::from_tree(&tree(), -7, SummaryOptions::default()),
            Summary::from_tree(&tree(), 1, opts),
        ] {
            let buf = s.encode();
            assert_eq!(Summary::decode(&buf).unwrap(), s);
            assert!(Summary::decode(&buf[..buf.len() - 1]).is_err());
        }
        assert_eq!(Summary::from_tree(&tree(), 1, opts).entries.len(), 9);
    }
}
