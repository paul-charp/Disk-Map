//! Arena tree of scanned file-system entries.

/// Index of a node in a [`Tree`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeId(pub u32);

const NONE: u32 = u32::MAX;

/// Kind of a tree node.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum NodeKind {
    File,
    Dir,
    Root,
}

/// Hot per-node data. Directory sizes are subtree totals.
#[derive(Clone, Debug)]
pub struct Node {
    parent: u32,
    first_child: u32,
    next_sibling: u32,
    name_off: u32,
    name_len: u32,
    /// Node kind.
    pub kind: NodeKind,
    /// Raw Windows file attributes.
    pub attrs: u32,
    child_count: u32,
    /// Logical size in bytes.
    pub logical: u64,
    /// Allocated size in bytes.
    pub allocated: u64,
}

impl Node {
    /// Number of direct children.
    pub fn child_count(&self) -> u32 {
        self.child_count
    }
}

/// Cold per-node timestamps (Windows FILETIME ticks).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Times {
    pub ctime: i64,
    pub mtime: i64,
    pub atime: i64,
}

/// Description of an entry to insert with [`Tree::add_child`].
#[derive(Clone, Copy, Debug)]
pub struct NewEntry<'a> {
    pub name: &'a str,
    pub kind: NodeKind,
    pub attrs: u32,
    pub logical: u64,
    pub allocated: u64,
    pub times: Times,
}

/// Arena tree with one shared name buffer.
#[derive(Debug)]
pub struct Tree {
    nodes: Vec<Node>,
    times: Vec<Times>,
    names: String,
    file_count: u64,
    dir_count: u64,
}

impl Tree {
    /// Creates a tree whose root node is named `root_path`.
    pub fn new(root_path: &str) -> Self {
        let mut tree = Tree {
            nodes: Vec::new(),
            times: Vec::new(),
            names: String::new(),
            file_count: 0,
            dir_count: 0,
        };
        let (off, len) = tree.push_name(root_path);
        tree.nodes.push(Node {
            parent: NONE,
            first_child: NONE,
            next_sibling: NONE,
            name_off: off,
            name_len: len,
            kind: NodeKind::Root,
            attrs: 0,
            child_count: 0,
            logical: 0,
            allocated: 0,
        });
        tree.times.push(Times::default());
        tree
    }

    fn push_name(&mut self, name: &str) -> (u32, u32) {
        let off = self.names.len() as u32;
        self.names.push_str(name);
        (off, name.len() as u32)
    }

    /// The root node.
    pub fn root(&self) -> NodeId {
        NodeId(0)
    }

    /// Number of nodes, root included.
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Always false: a tree has a root.
    pub fn is_empty(&self) -> bool {
        false
    }

    /// Number of files (not directories) added so far.
    pub fn file_count(&self) -> u64 {
        self.file_count
    }

    /// Number of directories added so far, root excluded.
    pub fn dir_count(&self) -> u64 {
        self.dir_count
    }

    /// Bytes used by the name arena.
    pub fn names_bytes(&self) -> usize {
        self.names.len()
    }

    /// Hot data of a node.
    pub fn get(&self, id: NodeId) -> &Node {
        &self.nodes[id.0 as usize]
    }

    /// Timestamps of a node.
    pub fn times(&self, id: NodeId) -> &Times {
        &self.times[id.0 as usize]
    }

    /// Name of a node; the root's name is the scan root path.
    pub fn name(&self, id: NodeId) -> &str {
        let node = self.get(id);
        let start = node.name_off as usize;
        &self.names[start..start + node.name_len as usize]
    }

    /// Parent of a node, `None` for the root.
    pub fn parent(&self, id: NodeId) -> Option<NodeId> {
        let p = self.get(id).parent;
        (p != NONE).then_some(NodeId(p))
    }

    /// Iterates the direct children of a node (most recently added first).
    pub fn children(&self, id: NodeId) -> Children<'_> {
        Children {
            tree: self,
            next: self.get(id).first_child,
        }
    }

    /// Full path of a node, joined with `\`.
    pub fn full_path(&self, id: NodeId) -> String {
        let mut parts = Vec::new();
        let mut cur = Some(id);
        while let Some(c) = cur {
            parts.push(self.name(c));
            cur = self.parent(c);
        }
        let mut out = String::new();
        for (i, part) in parts.iter().rev().enumerate() {
            if i > 0 && !out.ends_with('\\') {
                out.push('\\');
            }
            out.push_str(part);
        }
        out
    }

    /// Adds a child under `parent` and propagates its sizes up the ancestor chain.
    pub fn add_child(&mut self, parent: NodeId, entry: NewEntry<'_>) -> NodeId {
        let id = self.nodes.len() as u32;
        let (name_off, name_len) = self.push_name(entry.name);
        let first = self.nodes[parent.0 as usize].first_child;
        self.nodes.push(Node {
            parent: parent.0,
            first_child: NONE,
            next_sibling: first,
            name_off,
            name_len,
            kind: entry.kind,
            attrs: entry.attrs,
            child_count: 0,
            logical: entry.logical,
            allocated: entry.allocated,
        });
        self.times.push(entry.times);
        let p = &mut self.nodes[parent.0 as usize];
        p.first_child = id;
        p.child_count += 1;
        match entry.kind {
            NodeKind::File => self.file_count += 1,
            NodeKind::Dir => self.dir_count += 1,
            NodeKind::Root => {}
        }
        if entry.logical != 0 || entry.allocated != 0 {
            let mut cur = parent.0;
            while cur != NONE {
                let n = &mut self.nodes[cur as usize];
                n.logical += entry.logical;
                n.allocated += entry.allocated;
                cur = n.parent;
            }
        }
        NodeId(id)
    }

    /// Finds a direct child by name, ignoring case.
    pub fn find_child(&self, parent: NodeId, name: &str) -> Option<NodeId> {
        self.children(parent)
            .find(|&c| names_equal(self.name(c), name))
    }

    /// Resolves a path relative to the root (components separated by `\`); empty means the root.
    pub fn find_path(&self, relative: &str) -> Option<NodeId> {
        let mut cur = self.root();
        for part in relative.split('\\').filter(|p| !p.is_empty()) {
            cur = self.find_child(cur, part)?;
        }
        Some(cur)
    }

    /// Updates a file's sizes, times and attributes, propagating the size delta to its ancestors.
    /// For other node kinds only times and attributes change, since their sizes are totals.
    pub fn set_file(&mut self, id: NodeId, logical: u64, allocated: u64, times: Times, attrs: u32) {
        self.times[id.0 as usize] = times;
        let n = &mut self.nodes[id.0 as usize];
        n.attrs = attrs;
        if n.kind != NodeKind::File {
            return;
        }
        let (old_logical, old_allocated) = (n.logical, n.allocated);
        n.logical = logical;
        n.allocated = allocated;
        let mut cur = n.parent;
        while cur != NONE {
            let a = &mut self.nodes[cur as usize];
            a.logical = (a.logical + logical).saturating_sub(old_logical);
            a.allocated = (a.allocated + allocated).saturating_sub(old_allocated);
            cur = a.parent;
        }
    }

    /// Detaches a node and its subtree and subtracts their totals from all ancestors.
    /// Returns false for the root. The detached nodes stay in the arena, unreachable.
    pub fn remove(&mut self, id: NodeId) -> bool {
        let Some(parent) = self.parent(id) else {
            return false;
        };
        let (_, _, mut files, mut dirs) = self.subtree_totals(id);
        let n = self.get(id);
        let (logical, allocated, next) = (n.logical, n.allocated, n.next_sibling);
        match n.kind {
            NodeKind::File => files += 1,
            NodeKind::Dir => dirs += 1,
            NodeKind::Root => {}
        }
        let first = self.nodes[parent.0 as usize].first_child;
        if first == id.0 {
            self.nodes[parent.0 as usize].first_child = next;
        } else {
            let mut cur = first;
            while cur != NONE {
                let sib = self.nodes[cur as usize].next_sibling;
                if sib == id.0 {
                    self.nodes[cur as usize].next_sibling = next;
                    break;
                }
                cur = sib;
            }
        }
        let gone = &mut self.nodes[id.0 as usize];
        gone.next_sibling = NONE;
        gone.parent = NONE;
        self.nodes[parent.0 as usize].child_count -= 1;
        let mut cur = parent.0;
        while cur != NONE {
            let a = &mut self.nodes[cur as usize];
            a.logical = a.logical.saturating_sub(logical);
            a.allocated = a.allocated.saturating_sub(allocated);
            cur = a.parent;
        }
        self.file_count -= files;
        self.dir_count -= dirs;
        true
    }

    fn subtree_totals(&self, id: NodeId) -> (u64, u64, u64, u64) {
        let (mut logical, mut allocated, mut files, mut dirs) = (0, 0, 0, 0);
        for child in self.children(id) {
            let n = self.get(child);
            logical += n.logical;
            allocated += n.allocated;
            let (_, _, f, d) = self.subtree_totals(child);
            files += f + u64::from(n.kind == NodeKind::File);
            dirs += d + u64::from(n.kind == NodeKind::Dir);
        }
        (logical, allocated, files, dirs)
    }

    /// Detaches all children of `id` and subtracts their sizes from `id` and its ancestors.
    /// The detached nodes stay in the arena, unreachable.
    pub fn clear_children(&mut self, id: NodeId) {
        let (logical, allocated, files, dirs) = self.subtree_totals(id);
        let mut cur = id.0;
        while cur != NONE {
            let n = &mut self.nodes[cur as usize];
            n.logical = n.logical.saturating_sub(logical);
            n.allocated = n.allocated.saturating_sub(allocated);
            cur = n.parent;
        }
        let n = &mut self.nodes[id.0 as usize];
        n.first_child = NONE;
        n.child_count = 0;
        self.file_count -= files;
        self.dir_count -= dirs;
    }

    /// Copies everything below `src_root` of `src` under `parent`, adding sizes up the ancestor chain.
    pub fn graft_children(&mut self, parent: NodeId, src: &Tree, src_root: NodeId) {
        let mut stack = vec![(src_root, parent)];
        while let Some((from, to)) = stack.pop() {
            for child in src.children(from) {
                let n = src.get(child);
                let below: (u64, u64) = src.children(child).fold((0, 0), |a, c| {
                    (a.0 + src.get(c).logical, a.1 + src.get(c).allocated)
                });
                let id = self.add_child(
                    to,
                    NewEntry {
                        name: src.name(child),
                        kind: n.kind,
                        attrs: n.attrs,
                        logical: n.logical.saturating_sub(below.0),
                        allocated: n.allocated.saturating_sub(below.1),
                        times: *src.times(child),
                    },
                );
                stack.push((child, id));
            }
        }
    }
}

pub(crate) fn names_equal(a: &str, b: &str) -> bool {
    if a.is_ascii() && b.is_ascii() {
        a.eq_ignore_ascii_case(b)
    } else {
        a.to_lowercase() == b.to_lowercase()
    }
}

/// Iterator over the children of a node.
pub struct Children<'a> {
    tree: &'a Tree,
    next: u32,
}

impl Iterator for Children<'_> {
    type Item = NodeId;

    fn next(&mut self) -> Option<NodeId> {
        if self.next == NONE {
            return None;
        }
        let id = NodeId(self.next);
        self.next = self.tree.get(id).next_sibling;
        Some(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn entry(name: &str, kind: NodeKind, size: u64) -> NewEntry<'_> {
        NewEntry {
            name,
            kind,
            attrs: 0,
            logical: size,
            allocated: size.next_multiple_of(4096),
            times: Times::default(),
        }
    }

    #[test]
    fn node_size() {
        assert!(std::mem::size_of::<Node>() <= 72);
    }

    #[test]
    fn aggregation_and_paths() {
        let mut t = Tree::new("C:\\scan");
        let root = t.root();
        let a = t.add_child(root, entry("a", NodeKind::Dir, 0));
        let b = t.add_child(a, entry("b", NodeKind::Dir, 0));
        let f1 = t.add_child(b, entry("f1.txt", NodeKind::File, 100));
        let f2 = t.add_child(a, entry("f2.bin", NodeKind::File, 5000));
        t.add_child(root, entry("empty", NodeKind::Dir, 0));

        assert_eq!(t.get(b).logical, 100);
        assert_eq!(t.get(b).allocated, 4096);
        assert_eq!(t.get(a).logical, 5100);
        assert_eq!(t.get(a).allocated, 4096 + 8192);
        assert_eq!(t.get(root).logical, 5100);
        assert_eq!(t.file_count(), 2);
        assert_eq!(t.dir_count(), 3);
        assert_eq!(t.len(), 6);
        assert_eq!(t.get(root).child_count(), 2);

        let kids: HashSet<_> = t.children(a).collect();
        assert_eq!(kids, HashSet::from([b, f2]));
        assert_eq!(t.parent(f1), Some(b));
        assert_eq!(t.parent(root), None);

        assert_eq!(t.full_path(f1), "C:\\scan\\a\\b\\f1.txt");
        assert_eq!(t.full_path(root), "C:\\scan");
        assert_eq!(t.name(f2), "f2.bin");
    }

    #[test]
    fn replace_subtree_keeps_ancestor_sizes() {
        let mut t = Tree::new("C:\\scan");
        let root = t.root();
        let a = t.add_child(root, entry("a", NodeKind::Dir, 0));
        let b = t.add_child(a, entry("b", NodeKind::Dir, 0));
        t.add_child(b, entry("old.bin", NodeKind::File, 100));
        t.add_child(a, entry("keep.bin", NodeKind::File, 5000));
        t.add_child(root, entry("other.bin", NodeKind::File, 700));

        let mut sub = Tree::new("C:\\scan\\a\\b");
        let d = sub.add_child(sub.root(), entry("d", NodeKind::Dir, 0));
        sub.add_child(d, entry("n1.bin", NodeKind::File, 9000));
        sub.add_child(sub.root(), entry("n2.bin", NodeKind::File, 10));

        t.clear_children(b);
        assert_eq!(t.get(b).logical, 0);
        assert_eq!(t.get(a).logical, 5000);
        assert_eq!(t.get(root).logical, 5700);
        assert_eq!((t.file_count(), t.dir_count()), (2, 2));

        t.graft_children(b, &sub, sub.root());
        assert_eq!(t.get(b).logical, 9010);
        assert_eq!(t.get(b).allocated, 12288 + 4096);
        assert_eq!(t.get(a).logical, 14010);
        assert_eq!(t.get(root).logical, 14710);
        assert_eq!(
            t.get(root).allocated,
            4096 + 12288 + 4096 + 8192 + 4096 - 4096
        );
        assert_eq!((t.file_count(), t.dir_count()), (4, 3));
        assert_eq!(t.get(b).child_count(), 2);
        assert_eq!(
            t.full_path(t.children(b).find(|&c| t.name(c) == "n2.bin").unwrap()),
            "C:\\scan\\a\\b\\n2.bin"
        );
    }

    /// Recomputes every total from the reachable nodes and compares it with the stored ones.
    fn check_consistent(t: &Tree) {
        fn walk(t: &Tree, id: NodeId) -> (u64, u64, u64, u64) {
            let (mut l, mut a, mut f, mut d, mut count) = (0, 0, 0, 0, 0);
            for c in t.children(id) {
                count += 1;
                assert_eq!(t.parent(c), Some(id));
                let n = t.get(c);
                let (cl, ca, cf, cd) = walk(t, c);
                if n.kind == NodeKind::File {
                    l += n.logical;
                    a += n.allocated;
                    f += 1;
                } else {
                    assert_eq!((n.logical, n.allocated), (cl, ca), "dir {}", t.name(c));
                    l += cl;
                    a += ca;
                    f += cf;
                    d += cd + 1;
                }
            }
            assert_eq!(t.get(id).child_count(), count);
            (l, a, f, d)
        }
        let (l, a, f, d) = walk(t, t.root());
        let r = t.get(t.root());
        assert_eq!((r.logical, r.allocated), (l, a));
        assert_eq!((t.file_count(), t.dir_count()), (f, d));
    }

    fn reachable(t: &Tree, mut id: NodeId) -> bool {
        while let Some(p) = t.parent(id) {
            id = p;
        }
        id == t.root()
    }

    #[test]
    fn find_child_and_path() {
        let mut t = Tree::new("C:\\scan");
        let a = t.add_child(t.root(), entry("Alpha", NodeKind::Dir, 0));
        let f = t.add_child(a, entry("File.TXT", NodeKind::File, 10));
        assert_eq!(t.find_child(t.root(), "alpha"), Some(a));
        assert_eq!(t.find_child(t.root(), "beta"), None);
        assert_eq!(t.find_path("ALPHA\\file.txt"), Some(f));
        assert_eq!(t.find_path(""), Some(t.root()));
        assert_eq!(t.find_path("alpha\\nope"), None);
        let u = t.add_child(a, entry("\u{c9}t\u{e9}", NodeKind::File, 1));
        assert_eq!(t.find_child(a, "\u{e9}T\u{c9}"), Some(u));
    }

    #[test]
    fn set_file_propagates_delta() {
        let mut t = Tree::new("C:\\scan");
        let a = t.add_child(t.root(), entry("a", NodeKind::Dir, 0));
        let f = t.add_child(a, entry("f", NodeKind::File, 100));
        t.add_child(t.root(), entry("g", NodeKind::File, 50));
        let times = Times {
            ctime: 1,
            mtime: 2,
            atime: 3,
        };
        t.set_file(f, 9000, 12288, times, 0x20);
        assert_eq!((t.get(a).logical, t.get(a).allocated), (9000, 12288));
        assert_eq!(t.get(t.root()).logical, 9050);
        assert_eq!(*t.times(f), times);
        assert_eq!(t.get(f).attrs, 0x20);
        t.set_file(f, 10, 4096, times, 0x20);
        assert_eq!(t.get(t.root()).logical, 60);
        t.set_file(a, 5, 5, times, 0x10);
        assert_eq!(t.get(a).logical, 10);
        check_consistent(&t);
    }

    #[test]
    fn remove_keeps_totals() {
        let mut t = Tree::new("C:\\scan");
        let a = t.add_child(t.root(), entry("a", NodeKind::Dir, 0));
        let b = t.add_child(a, entry("b", NodeKind::Dir, 0));
        t.add_child(b, entry("f1", NodeKind::File, 100));
        let f2 = t.add_child(a, entry("f2", NodeKind::File, 5000));
        let f3 = t.add_child(a, entry("f3", NodeKind::File, 7));
        let g = t.add_child(t.root(), entry("g", NodeKind::File, 50));
        check_consistent(&t);
        assert!(t.remove(f2));
        check_consistent(&t);
        assert_eq!(t.children(a).count(), 2);
        assert!(t.remove(b));
        check_consistent(&t);
        assert_eq!((t.file_count(), t.dir_count()), (2, 1));
        assert_eq!(t.get(a).logical, 7);
        assert_eq!(t.find_path("a\\b"), None);
        assert!(!t.remove(b));
        assert!(t.remove(f3));
        assert!(t.remove(a));
        assert!(t.remove(g));
        check_consistent(&t);
        assert_eq!(t.get(t.root()).logical, 0);
        assert!(!t.remove(t.root()));
    }

    #[test]
    fn mutation_sequence_stays_consistent() {
        let mut t = Tree::new("C:\\scan");
        let mut files = Vec::new();
        let mut dirs = vec![t.root()];
        let mut seed = 12345u32;
        let mut rnd = |n: usize| {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            (seed >> 8) as usize % n
        };
        for i in 0..400 {
            match rnd(5) {
                0 => {
                    let p = dirs[rnd(dirs.len())];
                    dirs.push(t.add_child(p, entry(&format!("d{i}"), NodeKind::Dir, 0)));
                }
                1 | 2 => {
                    let p = dirs[rnd(dirs.len())];
                    let size = rnd(20000) as u64;
                    files.push(t.add_child(p, entry(&format!("f{i}"), NodeKind::File, size)));
                }
                3 if !files.is_empty() => {
                    let f = files[rnd(files.len())];
                    let size = rnd(30000) as u64;
                    t.set_file(f, size, size.next_multiple_of(4096), Times::default(), 0);
                }
                _ if dirs.len() > 1 => {
                    let d = dirs[1 + rnd(dirs.len() - 1)];
                    t.remove(d);
                    dirs.retain(|&x| reachable(&t, x));
                    files.retain(|&x| reachable(&t, x));
                }
                _ => {}
            }
            check_consistent(&t);
        }
    }

    #[test]
    fn drive_root_path() {
        let mut t = Tree::new("C:\\");
        let f = t.add_child(t.root(), entry("x", NodeKind::File, 1));
        assert_eq!(t.full_path(f), "C:\\x");
    }
}
