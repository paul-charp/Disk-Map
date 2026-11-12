//! Full snapshot of a [`Tree`] as a compact binary buffer. Format in `docs/snapshots.md`.

use crate::codec::{Reader, put_header, put_i64, put_str, put_u64};
use crate::tree::{NewEntry, NodeId, NodeKind, Times, Tree};

pub use crate::codec::Error;

const MAGIC: &[u8; 4] = b"DMSN";
const VERSION: u16 = 1;

const FLAG_ALL_TIMES: u64 = 1;
const FLAG_SECONDS: u64 = 2;

const TAG_DIR: u8 = 1;
const TAG_SAME_ATTRS: u8 = 2;
const ALLOC_SHIFT: u8 = 2;
const ALLOC_EXPLICIT: u8 = 0;
const ALLOC_ZERO: u8 = 1;
const ALLOC_CLUSTER: u8 = 2;
const ALLOC_LOGICAL: u8 = 3;
const TAG_SAME_MTIME: u8 = 16;
const TAG_PREFIX: u8 = 32;
const TAG_OWN_SIZE: u8 = 64;

const TICKS_PER_SECOND: i64 = 10_000_000;

/// Snapshot metadata and storage options.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SnapshotMeta {
    /// Scan time (FILETIME ticks).
    pub scan_time: i64,
    /// Cluster size used to predict allocated sizes; only affects the encoded size.
    pub cluster_size: u32,
    /// Keep creation and access times, not just the modification time.
    pub all_times: bool,
    /// Store times rounded down to whole seconds.
    pub seconds: bool,
}

impl Default for SnapshotMeta {
    fn default() -> Self {
        SnapshotMeta {
            scan_time: 0,
            cluster_size: 4096,
            all_times: false,
            seconds: false,
        }
    }
}

/// Header of a snapshot, readable without decoding the nodes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SnapshotInfo {
    /// Scan root path.
    pub root_path: String,
    /// Metadata and options.
    pub meta: SnapshotMeta,
    /// Nodes, root included.
    pub nodes: u64,
    /// Files.
    pub files: u64,
    /// Directories, root excluded.
    pub dirs: u64,
    /// Root logical size.
    pub logical: u64,
    /// Root allocated size.
    pub allocated: u64,
}

/// Encodes every node reachable from the root of `tree`.
pub fn encode(tree: &Tree, meta: &SnapshotMeta) -> Vec<u8> {
    let mut out = Vec::with_capacity(64 + tree.names_bytes() + tree.len() * 8);
    let root = tree.root();
    let r = tree.get(root);
    put_header(&mut out, MAGIC, VERSION);
    let flags = u64::from(meta.all_times) * FLAG_ALL_TIMES + u64::from(meta.seconds) * FLAG_SECONDS;
    put_u64(&mut out, flags);
    put_i64(&mut out, meta.scan_time);
    put_u64(&mut out, u64::from(meta.cluster_size));
    put_str(&mut out, tree.name(root));
    put_u64(&mut out, 1 + tree.file_count() + tree.dir_count());
    put_u64(&mut out, tree.file_count());
    put_u64(&mut out, tree.dir_count());
    put_u64(&mut out, r.logical);
    put_u64(&mut out, r.allocated);
    put_u64(&mut out, u64::from(r.attrs));
    let mut enc = Encoder {
        meta: *meta,
        out,
        prev_attrs: [0; 2],
        prev_mtime: 0,
    };
    enc.times(tree.times(root));
    let kids = children_in_order(tree, root);
    put_u64(&mut enc.out, kids.len() as u64);
    let mut stack = vec![(kids, 0usize, "")];
    while let Some((kids, next, prev)) = stack.last_mut() {
        let Some(&id) = kids.get(*next) else {
            stack.pop();
            continue;
        };
        *next += 1;
        let name = tree.name(id);
        let shared = shared_prefix(prev, name);
        *prev = name;
        let sub = enc.node(tree, id, name, shared);
        if let Some(sub) = sub {
            stack.push((sub, 0, ""));
        }
    }
    enc.out
}

/// Reads the header of a snapshot.
pub fn read_info(buf: &[u8]) -> Result<SnapshotInfo, Error> {
    Ok(read_head(buf)?.0)
}

/// Decodes a snapshot into a tree.
pub fn decode(buf: &[u8]) -> Result<(Tree, SnapshotMeta), Error> {
    let (info, mut r) = read_head(buf)?;
    let meta = info.meta;
    let mut tree = Tree::new(&info.root_path);
    let root = tree.root();
    let root_attrs = r.u32()?;
    let mut dec = Decoder {
        meta,
        prev_attrs: [0; 2],
        prev_mtime: 0,
    };
    let root_times = dec.times(&mut r)?;
    tree.set_file(root, 0, 0, root_times, root_attrs);
    let mut stack = vec![(root, r.u64()?, String::new())];
    let mut count = 1u64;
    while let Some((parent, left, prev)) = stack.last_mut() {
        if *left == 0 {
            stack.pop();
            continue;
        }
        *left -= 1;
        count += 1;
        if count > info.nodes {
            return Err(Error::Invalid("more nodes than the header says"));
        }
        let parent = *parent;
        let rec = dec.node(&mut r, prev)?;
        let id = tree.add_child(
            parent,
            NewEntry {
                name: prev,
                kind: rec.kind,
                attrs: rec.attrs,
                logical: rec.logical,
                allocated: rec.allocated,
                times: rec.times,
            },
        );
        if rec.kind == NodeKind::Dir {
            stack.push((id, rec.children, String::new()));
        }
    }
    if count != info.nodes || !r.is_at_end() {
        return Err(Error::Invalid("node count does not match the header"));
    }
    Ok((tree, meta))
}

fn read_head(buf: &[u8]) -> Result<(SnapshotInfo, Reader<'_>), Error> {
    let mut r = Reader::open(buf, MAGIC, VERSION)?;
    let flags = r.u64()?;
    let scan_time = r.i64()?;
    let cluster_size = r.u32()?;
    let root_path = r.str()?.to_owned();
    let info = SnapshotInfo {
        root_path,
        meta: SnapshotMeta {
            scan_time,
            cluster_size,
            all_times: flags & FLAG_ALL_TIMES != 0,
            seconds: flags & FLAG_SECONDS != 0,
        },
        nodes: r.u64()?,
        files: r.u64()?,
        dirs: r.u64()?,
        logical: r.u64()?,
        allocated: r.u64()?,
    };
    Ok((info, r))
}

/// Children in insertion order, so that decoding restores the sibling order.
fn children_in_order(tree: &Tree, id: NodeId) -> Vec<NodeId> {
    let mut kids: Vec<NodeId> = tree.children(id).collect();
    kids.reverse();
    kids
}

fn shared_prefix(a: &str, b: &str) -> usize {
    let mut n = a.bytes().zip(b.bytes()).take_while(|(x, y)| x == y).count();
    while !b.is_char_boundary(n) {
        n -= 1;
    }
    n
}

fn round_up(v: u64, cluster: u32) -> Option<u64> {
    (cluster > 0).then(|| v.checked_next_multiple_of(u64::from(cluster)))?
}

fn kind_slot(dir: bool) -> usize {
    usize::from(dir)
}

struct Encoder {
    meta: SnapshotMeta,
    out: Vec<u8>,
    prev_attrs: [u32; 2],
    prev_mtime: i64,
}

impl Encoder {
    fn time(&self, t: i64) -> i64 {
        if self.meta.seconds {
            t.div_euclid(TICKS_PER_SECOND)
        } else {
            t
        }
    }

    fn other_times(&mut self, t: &Times, m: i64) {
        if self.meta.all_times {
            let (c, a) = (self.time(t.ctime), self.time(t.atime));
            put_i64(&mut self.out, c.wrapping_sub(m));
            put_i64(&mut self.out, a.wrapping_sub(m));
        }
    }

    fn times(&mut self, t: &Times) {
        let m = self.time(t.mtime);
        put_i64(&mut self.out, m);
        self.other_times(t, m);
        self.prev_mtime = m;
    }

    /// Writes one node; returns its children when it is a directory.
    fn node(&mut self, tree: &Tree, id: NodeId, name: &str, shared: usize) -> Option<Vec<NodeId>> {
        let n = tree.get(id);
        let dir = n.kind != NodeKind::File;
        let kids = dir.then(|| children_in_order(tree, id));
        let (logical, allocated) = match &kids {
            Some(kids) => kids.iter().fold((n.logical, n.allocated), |(l, a), &c| {
                let c = tree.get(c);
                (l.saturating_sub(c.logical), a.saturating_sub(c.allocated))
            }),
            None => (n.logical, n.allocated),
        };
        let has_size = !dir || logical != 0 || allocated != 0;
        let alloc_mode = if allocated == 0 {
            ALLOC_ZERO
        } else if allocated == logical {
            ALLOC_LOGICAL
        } else if round_up(logical, self.meta.cluster_size) == Some(allocated) {
            ALLOC_CLUSTER
        } else {
            ALLOC_EXPLICIT
        };
        let slot = kind_slot(dir);
        let same_attrs = self.prev_attrs[slot] == n.attrs;
        let t = tree.times(id);
        let m = self.time(t.mtime);
        let mut tag = alloc_mode << ALLOC_SHIFT;
        tag |= if dir { TAG_DIR } else { 0 };
        tag |= if same_attrs { TAG_SAME_ATTRS } else { 0 };
        tag |= if m == self.prev_mtime {
            TAG_SAME_MTIME
        } else {
            0
        };
        tag |= if shared > 0 { TAG_PREFIX } else { 0 };
        tag |= if dir && has_size { TAG_OWN_SIZE } else { 0 };
        self.out.push(tag);
        if let Some(kids) = &kids {
            put_u64(&mut self.out, kids.len() as u64);
        }
        if !same_attrs {
            put_u64(&mut self.out, u64::from(n.attrs));
            self.prev_attrs[slot] = n.attrs;
        }
        if has_size {
            put_u64(&mut self.out, logical);
            if alloc_mode == ALLOC_EXPLICIT {
                put_u64(&mut self.out, allocated);
            }
        }
        if m != self.prev_mtime {
            put_i64(&mut self.out, m.wrapping_sub(self.prev_mtime));
            self.prev_mtime = m;
        }
        self.other_times(t, m);
        if shared > 0 {
            put_u64(&mut self.out, shared as u64);
        }
        put_str(&mut self.out, &name[shared..]);
        kids
    }
}

struct Record {
    kind: NodeKind,
    attrs: u32,
    children: u64,
    logical: u64,
    allocated: u64,
    times: Times,
}

struct Decoder {
    meta: SnapshotMeta,
    prev_attrs: [u32; 2],
    prev_mtime: i64,
}

impl Decoder {
    fn ticks(&self, t: i64) -> i64 {
        if self.meta.seconds {
            t.saturating_mul(TICKS_PER_SECOND)
        } else {
            t
        }
    }

    fn other_times(&self, r: &mut Reader<'_>, m: i64) -> Result<(i64, i64), Error> {
        if !self.meta.all_times {
            return Ok((0, 0));
        }
        let c = m.wrapping_add(r.i64()?);
        let a = m.wrapping_add(r.i64()?);
        Ok((self.ticks(c), self.ticks(a)))
    }

    fn times(&mut self, r: &mut Reader<'_>) -> Result<Times, Error> {
        let m = r.i64()?;
        self.prev_mtime = m;
        let (ctime, atime) = self.other_times(r, m)?;
        Ok(Times {
            ctime,
            mtime: self.ticks(m),
            atime,
        })
    }

    /// Reads one node; `name` holds the previous sibling's name and receives this one.
    fn node(&mut self, r: &mut Reader<'_>, name: &mut String) -> Result<Record, Error> {
        let tag = r.u8()?;
        if tag & 0x80 != 0 {
            return Err(Error::Invalid("unknown node flag"));
        }
        let dir = tag & TAG_DIR != 0;
        let children = if dir { r.u64()? } else { 0 };
        let slot = kind_slot(dir);
        if tag & TAG_SAME_ATTRS == 0 {
            self.prev_attrs[slot] = r.u32()?;
        }
        let has_size = !dir || tag & TAG_OWN_SIZE != 0;
        let (mut logical, mut allocated) = (0, 0);
        if has_size {
            logical = r.u64()?;
            allocated = match (tag >> ALLOC_SHIFT) & 3 {
                ALLOC_ZERO => 0,
                ALLOC_LOGICAL => logical,
                ALLOC_CLUSTER => round_up(logical, self.meta.cluster_size)
                    .ok_or(Error::Invalid("allocated size overflow"))?,
                _ => r.u64()?,
            };
        }
        if tag & TAG_SAME_MTIME == 0 {
            self.prev_mtime = self.prev_mtime.wrapping_add(r.i64()?);
        }
        let m = self.prev_mtime;
        let (ctime, atime) = self.other_times(r, m)?;
        let shared = if tag & TAG_PREFIX != 0 { r.u64()? } else { 0 };
        let shared = usize::try_from(shared).map_err(|_| Error::Invalid("bad name prefix"))?;
        if shared > name.len() || !name.is_char_boundary(shared) {
            return Err(Error::Invalid("bad name prefix"));
        }
        name.truncate(shared);
        name.push_str(r.str()?);
        Ok(Record {
            kind: if dir { NodeKind::Dir } else { NodeKind::File },
            attrs: self.prev_attrs[slot],
            children,
            logical,
            allocated,
            times: Times {
                ctime,
                mtime: self.ticks(m),
                atime,
            },
        })
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn file(name: &str, size: u64, mtime: i64) -> NewEntry<'_> {
        NewEntry {
            name,
            kind: NodeKind::File,
            attrs: 0x20,
            logical: size,
            allocated: size.next_multiple_of(4096),
            times: Times {
                ctime: mtime - 5,
                mtime,
                atime: mtime + 7,
            },
        }
    }

    pub(crate) fn dir(name: &str) -> NewEntry<'_> {
        NewEntry {
            name,
            kind: NodeKind::Dir,
            attrs: 0x10,
            logical: 0,
            allocated: 0,
            times: Times::default(),
        }
    }

    pub(crate) fn sample() -> Tree {
        let mut t = Tree::new("C:\\data");
        let root = t.root();
        let a = t.add_child(root, dir("Alpha"));
        let b = t.add_child(a, dir("Beta"));
        t.add_child(b, file("IMG_0001.jpg", 1_234_567, 133_000_000_000_000_000));
        t.add_child(b, file("IMG_0002.jpg", 2_345_678, 133_000_000_000_100_000));
        t.add_child(b, file("\u{e9}t\u{e9}.txt", 10, 133_000_000_000_100_000));
        let odd = t.add_child(a, file("sparse.bin", 1 << 40, -3));
        t.set_file(odd, 1 << 40, 65536, *t.times(odd), 0x220);
        let small = t.add_child(a, file("tiny", 100, 0));
        t.set_file(small, 100, 0, Times::default(), 0x20);
        t.add_child(root, dir("empty"));
        t.add_child(root, file("top.bin", 0, 1));
        let own = t.add_child(
            root,
            NewEntry {
                logical: 77,
                allocated: 4096,
                ..dir("withsize")
            },
        );
        t.add_child(own, file("x", 3, 2));
        t.set_file(
            root,
            0,
            0,
            Times {
                ctime: 1,
                mtime: 2,
                atime: 3,
            },
            0x16,
        );
        t
    }

    /// Asserts that two trees hold the same nodes in the same order.
    pub(crate) fn assert_same(a: &Tree, b: &Tree, times: impl Fn(&Times) -> Times) {
        fn walk(a: &Tree, x: NodeId, b: &Tree, y: NodeId, times: &dyn Fn(&Times) -> Times) {
            let (n, m) = (a.get(x), b.get(y));
            assert_eq!(a.name(x), b.name(y));
            assert_eq!(
                (n.kind, n.attrs, n.logical, n.allocated, n.child_count()),
                (m.kind, m.attrs, m.logical, m.allocated, m.child_count()),
                "{}",
                a.full_path(x)
            );
            assert_eq!(times(a.times(x)), *b.times(y), "{}", a.full_path(x));
            for (c, d) in a.children(x).zip(b.children(y)) {
                walk(a, c, b, d, times);
            }
        }
        walk(a, a.root(), b, b.root(), &times);
        assert_eq!(
            (a.file_count(), a.dir_count()),
            (b.file_count(), b.dir_count())
        );
    }

    #[test]
    fn round_trip_all_options() {
        let t = sample();
        for (all_times, seconds) in [(false, false), (true, false), (true, true), (false, true)] {
            let meta = SnapshotMeta {
                scan_time: 133_500_000_000_000_000,
                cluster_size: 4096,
                all_times,
                seconds,
            };
            let buf = encode(&t, &meta);
            let (back, m) = decode(&buf).unwrap();
            assert_eq!(m, meta);
            let q = |v: i64| {
                if seconds {
                    v.div_euclid(TICKS_PER_SECOND) * TICKS_PER_SECOND
                } else {
                    v
                }
            };
            assert_same(&t, &back, |x| Times {
                ctime: if all_times { q(x.ctime) } else { 0 },
                mtime: q(x.mtime),
                atime: if all_times { q(x.atime) } else { 0 },
            });
            let info = read_info(&buf).unwrap();
            assert_eq!(info.root_path, "C:\\data");
            assert_eq!(info.nodes, t.len() as u64);
            assert_eq!(info.logical, t.get(t.root()).logical);
        }
    }

    #[test]
    fn skips_removed_nodes() {
        let mut t = sample();
        let gone = t.find_path("Alpha\\Beta").unwrap();
        t.remove(gone);
        let (back, _) = decode(&encode(&t, &SnapshotMeta::default())).unwrap();
        assert_eq!(back.len() as u64, 1 + t.file_count() + t.dir_count());
        assert_eq!(back.find_path("Alpha\\Beta"), None);
        assert_eq!(back.get(back.root()).allocated, t.get(t.root()).allocated);
    }

    #[test]
    fn rejects_bad_input() {
        let buf = encode(&sample(), &SnapshotMeta::default());
        assert_eq!(decode(b"nope").unwrap_err(), Error::BadMagic);
        for cut in [7, buf.len() / 2, buf.len() - 1] {
            assert!(decode(&buf[..cut]).is_err(), "cut at {cut}");
        }
        let mut extra = buf.clone();
        extra.push(0);
        assert!(decode(&extra).is_err());
        let mut v2 = buf;
        v2[4] = 2;
        assert_eq!(decode(&v2).unwrap_err(), Error::Version(2));
    }

    #[test]
    fn prefix_respects_char_boundaries() {
        assert_eq!(shared_prefix("\u{e9}a", "\u{e8}a"), 0);
        assert_eq!(shared_prefix("ab\u{e9}", "ab\u{e8}"), 2);
        assert_eq!(shared_prefix("abc", "abd"), 2);
    }
}
