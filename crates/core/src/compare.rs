//! Per-node comparison of the current tree against an older one, for compare mode.

use std::collections::HashMap;

use crate::codec::fold;
use crate::diff::Status;
use crate::tree::{NodeId, NodeKind, Tree, names_equal};

const NO_OLD: u64 = u64::MAX;

/// Delta class of an unchanged item.
pub const CLASS_SAME: u8 = 0;
/// Delta class of an item missing from the old tree.
pub const CLASS_NEW: u8 = 1;
/// Delta class of an item that shrank.
pub const CLASS_SHRUNK: u8 = 2;
/// First of the grown classes, ramped by relative growth up to `CLASS_GROWN + GROWN_STEPS - 1`.
pub const CLASS_GROWN: u8 = 3;
/// Number of grown classes.
pub const GROWN_STEPS: u8 = 4;
/// Delta class of the deleted pseudo-item.
pub const CLASS_DELETED: u8 = CLASS_GROWN + GROWN_STEPS;

/// Growth relative to the old size at which each grown step after the first starts.
const GROWN_RAMP: [f64; 3] = [0.1, 0.5, 1.0];

/// Old sizes of the nodes of a current tree, matched by path, and the bytes deleted below each folder.
#[derive(Clone, Debug, Default)]
pub struct Compare {
    old: Vec<u64>,
    deleted: Vec<u64>,
    below: Vec<u64>,
    counts: HashMap<u32, u32>,
}

impl Compare {
    /// Matches `new` against `old` by child name, case-insensitively.
    pub fn new(old: &Tree, new: &Tree) -> Self {
        let n = new.len();
        let mut c = Compare {
            old: vec![NO_OLD; n],
            deleted: vec![0; n],
            below: vec![0; n],
            counts: HashMap::new(),
        };
        let mut stack = vec![(old.root(), new.root())];
        c.old[new.root().0 as usize] = old.get(old.root()).allocated;
        while let Some((o, nw)) = stack.pop() {
            c.pair_children(old, new, o, nw, &mut stack);
        }
        c.sum_below(new);
        c
    }

    fn pair_children(
        &mut self,
        old: &Tree,
        new: &Tree,
        o: NodeId,
        n: NodeId,
        stack: &mut Vec<(NodeId, NodeId)>,
    ) {
        let olds: Vec<NodeId> = old.children(o).collect();
        let mut used = vec![false; olds.len()];
        let lookup = (olds.len() > 16).then(|| name_index(old, &olds));
        for c in new.children(n) {
            let name = new.name(c);
            let from = match &lookup {
                Some(m) => m.get(&fold(name)).copied(),
                None => Some(0),
            };
            let found = from.and_then(|from| {
                (from..olds.len()).find(|&i| !used[i] && names_equal(old.name(olds[i]), name))
            });
            let Some(i) = found else { continue };
            if is_dir(old, olds[i]) != is_dir(new, c) {
                continue;
            }
            used[i] = true;
            self.old[c.0 as usize] = old.get(olds[i]).allocated;
            if is_dir(new, c) {
                stack.push((olds[i], c));
            }
        }
        for (i, &oc) in olds.iter().enumerate() {
            if !used[i] {
                self.deleted[n.0 as usize] += old.get(oc).allocated;
                *self.counts.entry(n.0).or_default() += 1;
            }
        }
    }

    /// Adds each folder's deleted bytes to all its ancestors. Children have larger ids than parents.
    fn sum_below(&mut self, new: &Tree) {
        self.below.copy_from_slice(&self.deleted);
        for i in (1..self.below.len()).rev() {
            let v = self.below[i];
            if v == 0 {
                continue;
            }
            if let Some(p) = new.parent(NodeId(i as u32)) {
                self.below[p.0 as usize] += v;
            }
        }
    }

    /// Allocated size in the old tree, `None` for a node that is new (or added after the comparison).
    pub fn old_size(&self, id: NodeId) -> Option<u64> {
        self.old
            .get(id.0 as usize)
            .copied()
            .filter(|&v| v != NO_OLD)
    }

    /// Change of a node, judged by allocated size.
    pub fn status(&self, tree: &Tree, id: NodeId) -> Status {
        let Some(old) = self.old_size(id) else {
            return Status::New;
        };
        let new = tree.get(id).allocated;
        match new.cmp(&old) {
            std::cmp::Ordering::Equal => Status::Same,
            std::cmp::Ordering::Greater => Status::Grown,
            std::cmp::Ordering::Less => Status::Shrunk,
        }
    }

    /// Allocated size change of a node in bytes; a new node counts in full.
    pub fn delta(&self, tree: &Tree, id: NodeId) -> i64 {
        let new = i128::from(tree.get(id).allocated);
        let old = i128::from(self.old_size(id).unwrap_or(0));
        (new - old).clamp(i64::MIN.into(), i64::MAX.into()) as i64
    }

    /// Delta class of a node for colouring (see the `CLASS_*` constants).
    pub fn class(&self, tree: &Tree, id: NodeId) -> u8 {
        match self.status(tree, id) {
            Status::Same => CLASS_SAME,
            Status::New => CLASS_NEW,
            Status::Shrunk => CLASS_SHRUNK,
            Status::Deleted => CLASS_DELETED,
            Status::Grown => {
                let old = self.old_size(id).unwrap_or(0).max(1) as f64;
                let ratio = self.delta(tree, id) as f64 / old;
                CLASS_GROWN + GROWN_RAMP.iter().filter(|&&r| ratio >= r).count() as u8
            }
        }
    }

    /// Allocated bytes of the old children of a folder that have no match.
    pub fn deleted(&self, id: NodeId) -> u64 {
        self.deleted.get(id.0 as usize).copied().unwrap_or(0)
    }

    /// Number of old children of a folder that have no match.
    pub fn deleted_items(&self, id: NodeId) -> u32 {
        self.counts.get(&id.0).copied().unwrap_or(0)
    }

    /// Deleted bytes directly in each folder, indexed by node id.
    pub fn deleted_slice(&self) -> &[u64] {
        &self.deleted
    }

    /// Deleted bytes anywhere below each folder, indexed by node id.
    pub fn below_slice(&self) -> &[u64] {
        &self.below
    }
}

fn is_dir(tree: &Tree, id: NodeId) -> bool {
    tree.get(id).kind != NodeKind::File
}

/// First index of each folded name.
fn name_index(tree: &Tree, ids: &[NodeId]) -> HashMap<String, usize> {
    let mut m = HashMap::with_capacity(ids.len());
    for (i, &c) in ids.iter().enumerate().rev() {
        m.insert(fold(tree.name(c)), i);
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snapshot::tests::{dir, file};

    fn trees() -> (Tree, Tree) {
        let mut old = Tree::new("C:\\");
        let r = old.root();
        let docs = old.add_child(r, dir("Docs"));
        old.add_child(docs, file("a.txt", 10_000, 0));
        old.add_child(docs, file("gone.txt", 8192, 0));
        let gone = old.add_child(r, dir("Gone"));
        old.add_child(gone, file("x", 50_000, 0));
        old.add_child(r, file("same", 4096, 0));
        old.add_child(r, file("flip", 1, 0));

        let mut new = Tree::new("C:\\");
        let r = new.root();
        let docs = new.add_child(r, dir("docs"));
        new.add_child(docs, file("A.txt", 40_000, 0));
        new.add_child(docs, file("fresh", 100, 0));
        new.add_child(r, file("same", 4096, 0));
        new.add_child(r, dir("flip"));
        (old, new)
    }

    #[test]
    fn matches_by_name_and_sums_deleted() {
        let (old, new) = trees();
        let c = Compare::new(&old, &new);
        let find = |p: &str| new.find_path(p).expect(p);
        assert_eq!(c.status(&new, find("docs")), Status::Grown);
        assert_eq!(c.status(&new, find("docs\\A.txt")), Status::Grown);
        assert_eq!(c.status(&new, find("docs\\fresh")), Status::New);
        assert_eq!(c.status(&new, find("same")), Status::Same);
        assert_eq!(c.status(&new, find("flip")), Status::New);
        assert_eq!(c.deleted(find("docs")), 8192);
        assert_eq!(c.deleted_items(new.root()), 2);
        let gone = 53_248 + 4096;
        assert_eq!(c.deleted(new.root()), gone);
        assert_eq!(c.below_slice()[0], gone + 8192);
        assert_eq!(c.delta(&new, find("docs\\fresh")), 4096);
    }

    #[test]
    fn classes_ramp_by_growth() {
        let (old, new) = trees();
        let c = Compare::new(&old, &new);
        let a = new.find_path("docs\\A.txt").expect("a");
        assert_eq!(c.class(&new, a), CLASS_GROWN + 3);
        assert_eq!(c.class(&new, new.find_path("same").expect("s")), CLASS_SAME);
        assert_eq!(c.old_size(NodeId(999)), None);
    }
}
