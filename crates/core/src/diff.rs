//! Differences between two trees or two timeline points, matched by path.

use std::collections::HashMap;

use crate::codec::fold;
use crate::summary::Summary;
use crate::tree::{NodeId, NodeKind, Tree, names_equal};

/// Allocated and logical sizes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Sizes {
    /// Allocated bytes.
    pub allocated: u64,
    /// Logical bytes.
    pub logical: u64,
}

/// How an entry changed, judged by allocated size.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    /// Only in the new side.
    New,
    /// Only in the old side.
    Deleted,
    /// Larger than before.
    Grown,
    /// Smaller than before.
    Shrunk,
    /// Within [`DiffOptions::same_within`] of the old size.
    Same,
}

/// One changed (or kept) path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiffEntry {
    /// Path relative to the root; empty for the root.
    pub path: String,
    /// Index of the parent entry; `None` for the root.
    pub parent: Option<u32>,
    /// Folder (or root) rather than file.
    pub dir: bool,
    /// Change status.
    pub status: Status,
    /// Old sizes, zero when absent.
    pub old: Sizes,
    /// New sizes, zero when absent.
    pub new: Sizes,
    /// One side is missing from a timeline point because it was below the size threshold,
    /// so its size is taken as zero.
    pub approx: bool,
}

impl DiffEntry {
    /// Allocated size change in bytes.
    pub fn delta(&self) -> i64 {
        (i128::from(self.new.allocated) - i128::from(self.old.allocated))
            .clamp(i64::MIN.into(), i64::MAX.into()) as i64
    }

    /// Logical size change in bytes.
    pub fn logical_delta(&self) -> i64 {
        (i128::from(self.new.logical) - i128::from(self.old.logical))
            .clamp(i64::MIN.into(), i64::MAX.into()) as i64
    }
}

/// What a diff reports.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DiffOptions {
    /// Size changes of at most this many allocated bytes count as [`Status::Same`].
    pub same_within: u64,
    /// List files too, not only folders.
    pub files: bool,
    /// List unchanged entries too.
    pub unchanged: bool,
}

fn status(old: Sizes, new: Sizes, same_within: u64) -> Status {
    if old.allocated.abs_diff(new.allocated) <= same_within {
        Status::Same
    } else if new.allocated > old.allocated {
        Status::Grown
    } else {
        Status::Shrunk
    }
}

fn join(base: &str, name: &str) -> String {
    if base.is_empty() {
        name.to_owned()
    } else {
        format!("{base}\\{name}")
    }
}

/// Diffs two full trees. Entries are in pre-order, the root first. A new or deleted folder is
/// one entry with its totals; its contents are not listed. An unchanged folder is listed
/// when something below it is.
pub fn diff_trees(old: &Tree, new: &Tree, options: &DiffOptions) -> Vec<DiffEntry> {
    let mut d = TreeDiff {
        old,
        new,
        options: *options,
        out: Vec::new(),
    };
    d.pair(old.root(), new.root(), String::new(), None);
    d.out
}

struct TreeDiff<'a> {
    old: &'a Tree,
    new: &'a Tree,
    options: DiffOptions,
    out: Vec<DiffEntry>,
}

fn sizes(tree: &Tree, id: NodeId) -> Sizes {
    let n = tree.get(id);
    Sizes {
        allocated: n.allocated,
        logical: n.logical,
    }
}

fn is_dir(tree: &Tree, id: NodeId) -> bool {
    tree.get(id).kind != NodeKind::File
}

impl TreeDiff<'_> {
    fn wanted(&self, dir: bool) -> bool {
        dir || self.options.files
    }

    /// Emits one side-only entry.
    fn single(&mut self, tree: &Tree, id: NodeId, path: String, parent: Option<u32>, old: bool) {
        let dir = is_dir(tree, id);
        if !self.wanted(dir) {
            return;
        }
        let s = sizes(tree, id);
        let (status, old_s, new_s) = if old {
            (Status::Deleted, s, Sizes::default())
        } else {
            (Status::New, Sizes::default(), s)
        };
        self.out.push(DiffEntry {
            path,
            parent,
            dir,
            status,
            old: old_s,
            new: new_s,
            approx: false,
        });
    }

    /// Emits a node present on both sides and, for folders, its children. Returns whether
    /// anything was kept.
    fn pair(&mut self, o: NodeId, n: NodeId, path: String, parent: Option<u32>) -> bool {
        let (od, nd) = (is_dir(self.old, o), is_dir(self.new, n));
        if od != nd {
            self.single(self.old, o, path.clone(), parent, true);
            self.single(self.new, n, path, parent, false);
            return self.wanted(od) || self.wanted(nd);
        }
        if !self.wanted(od) {
            return false;
        }
        let (os, ns) = (sizes(self.old, o), sizes(self.new, n));
        let st = status(os, ns, self.options.same_within);
        let index = self.out.len();
        self.out.push(DiffEntry {
            path,
            parent,
            dir: od,
            status: st,
            old: os,
            new: ns,
            approx: false,
        });
        let mut below = false;
        if od {
            below = self.children(o, n, index as u32);
        }
        let keep = parent.is_none() || st != Status::Same || self.options.unchanged || below;
        if !keep {
            self.out.truncate(index);
        }
        keep
    }

    fn children(&mut self, o: NodeId, n: NodeId, index: u32) -> bool {
        let mut olds: Vec<NodeId> = self.old.children(o).collect();
        olds.reverse();
        let mut used = vec![false; olds.len()];
        let lookup: Option<HashMap<String, usize>> = (olds.len() > 16).then(|| {
            let mut m = HashMap::with_capacity(olds.len());
            for (i, &c) in olds.iter().enumerate().rev() {
                m.insert(fold(self.old.name(c)), i);
            }
            m
        });
        let mut news: Vec<NodeId> = self.new.children(n).collect();
        news.reverse();
        let mut any = false;
        for c in news {
            let name = self.new.name(c);
            // The map holds the first match; later ones (duplicate names) are found by scanning on.
            let from = match &lookup {
                Some(m) => m.get(&fold(name)).copied(),
                None => Some(0),
            };
            let found = from.and_then(|from| {
                (from..olds.len()).find(|&i| !used[i] && names_equal(self.old.name(olds[i]), name))
            });
            let path = join(&self.out[index as usize].path, name);
            if let Some(i) = found {
                used[i] = true;
                any |= self.pair(olds[i], c, path, Some(index));
            } else if self.wanted(is_dir(self.new, c)) {
                self.single(self.new, c, path, Some(index), false);
                any = true;
            }
        }
        for (i, &c) in olds.iter().enumerate() {
            if !used[i] && self.wanted(is_dir(self.old, c)) {
                let path = join(&self.out[index as usize].path, self.old.name(c));
                self.single(self.old, c, path, Some(index), true);
                any = true;
            }
        }
        any
    }
}

/// Diffs two timeline points. The root comes first and every parent before its children.
/// A folder missing from one point counts as new or deleted when that point would have kept
/// it by depth; otherwise it was below the size threshold and the entry is marked `approx`.
pub fn diff_summaries(old: &Summary, new: &Summary, options: &DiffOptions) -> Vec<DiffEntry> {
    let sz = |e: &crate::summary::SummaryEntry| Sizes {
        allocated: e.allocated,
        logical: e.logical,
    };
    let old_index: HashMap<String, usize> = old
        .entries
        .iter()
        .enumerate()
        .map(|(i, e)| (fold(&e.path), i))
        .collect();
    let mut out: Vec<DiffEntry> = Vec::new();
    let mut kept = Vec::new();
    // Per new entry: its diff index and whether it truly did not exist in the old point.
    let mut new_map: Vec<(Option<u32>, bool)> = Vec::with_capacity(new.entries.len());
    let mut old_map: Vec<Option<u32>> = vec![None; old.entries.len()];
    for e in &new.entries {
        let parent = e.parent.map(|p| new_map[p as usize]);
        let matched = old_index.get(&fold(&e.path)).copied();
        if let Some(i) = matched {
            let st = status(sz(&old.entries[i]), sz(e), options.same_within);
            old_map[i] = Some(out.len() as u32);
            new_map.push((Some(out.len() as u32), false));
            out.push(DiffEntry {
                path: e.path.clone(),
                parent: parent.and_then(|p| p.0),
                dir: true,
                status: st,
                old: sz(&old.entries[i]),
                new: sz(e),
                approx: false,
            });
            continue;
        }
        let truly = e.depth <= old.options.max_depth || parent.is_some_and(|p| p.1);
        let (st, approx) = if truly {
            (Status::New, false)
        } else {
            (Status::Grown, true)
        };
        new_map.push((Some(out.len() as u32), truly));
        out.push(DiffEntry {
            path: e.path.clone(),
            parent: parent.and_then(|p| p.0),
            dir: true,
            status: st,
            old: Sizes::default(),
            new: sz(e),
            approx,
        });
    }
    let mut truly_gone = vec![false; old.entries.len()];
    for (i, e) in old.entries.iter().enumerate() {
        if old_map[i].is_some() {
            continue;
        }
        let p = e.parent.map(|p| p as usize);
        let truly = e.depth <= new.options.max_depth || p.is_some_and(|p| truly_gone[p]);
        truly_gone[i] = truly;
        let (st, approx) = if truly {
            (Status::Deleted, false)
        } else {
            (Status::Shrunk, true)
        };
        old_map[i] = Some(out.len() as u32);
        out.push(DiffEntry {
            path: e.path.clone(),
            parent: p.and_then(|p| old_map[p]),
            dir: true,
            status: st,
            old: sz(e),
            new: Sizes::default(),
            approx,
        });
    }
    if options.unchanged {
        return out;
    }
    // Drop unchanged entries that have no kept descendant, keeping parents valid.
    let mut needed = vec![false; out.len()];
    for i in (0..out.len()).rev() {
        if i == 0 || out[i].status != Status::Same || needed[i] {
            needed[i] = true;
            if let Some(p) = out[i].parent {
                needed[p as usize] = true;
            }
        }
    }
    let mut remap = vec![0u32; out.len()];
    for (i, e) in out.into_iter().enumerate() {
        if needed[i] {
            remap[i] = kept.len() as u32;
            kept.push(DiffEntry {
                parent: e.parent.map(|p| remap[p as usize]),
                ..e
            });
        }
    }
    kept
}

/// Indexes of the entries that grew most, largest first. With `leaf_most`, an entry is skipped
/// when something listed below it also grew, so no growth is counted twice.
pub fn top_growers(entries: &[DiffEntry], n: usize, leaf_most: bool) -> Vec<usize> {
    movers(entries, n, leaf_most, 1)
}

/// Indexes of the entries that shrank most, largest first. See [`top_growers`].
pub fn top_shrinkers(entries: &[DiffEntry], n: usize, leaf_most: bool) -> Vec<usize> {
    movers(entries, n, leaf_most, -1)
}

fn movers(entries: &[DiffEntry], n: usize, leaf_most: bool, sign: i64) -> Vec<usize> {
    let moving = |e: &DiffEntry| {
        e.parent.is_some() && e.status != Status::Same && e.delta().signum() == sign
    };
    let mut covered = vec![false; entries.len()];
    if leaf_most {
        for e in entries.iter().filter(|e| moving(e)) {
            let mut p = e.parent;
            while let Some(i) = p {
                if covered[i as usize] {
                    break;
                }
                covered[i as usize] = true;
                p = entries[i as usize].parent;
            }
        }
    }
    let mut out: Vec<usize> = (0..entries.len())
        .filter(|&i| moving(&entries[i]) && !covered[i])
        .collect();
    out.sort_by_key(|&i| std::cmp::Reverse(entries[i].delta().unsigned_abs()));
    out.truncate(n);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snapshot::tests::{dir, file};
    use crate::summary::SummaryOptions;

    fn old_tree() -> Tree {
        let mut t = Tree::new("C:\\");
        let r = t.root();
        let docs = t.add_child(r, dir("Docs"));
        t.add_child(docs, file("a.txt", 10_000, 0));
        t.add_child(docs, file("b.txt", 20_000, 0));
        let gone = t.add_child(r, dir("Gone"));
        t.add_child(gone, file("x", 50_000, 0));
        let same = t.add_child(r, dir("Same"));
        let inner = t.add_child(same, dir("Inner"));
        t.add_child(inner, file("y", 4096, 0));
        t.add_child(r, file("flip", 1, 0));
        t
    }

    fn new_tree() -> Tree {
        let mut t = Tree::new("C:\\");
        let r = t.root();
        let docs = t.add_child(r, dir("docs"));
        t.add_child(docs, file("A.TXT", 90_000, 0));
        t.add_child(docs, file("c.txt", 1, 0));
        let fresh = t.add_child(r, dir("Fresh"));
        t.add_child(fresh, file("z", 9_000_000, 0));
        let same = t.add_child(r, dir("Same"));
        let inner = t.add_child(same, dir("Inner"));
        t.add_child(inner, file("y", 4096, 0));
        let flip = t.add_child(r, dir("flip"));
        t.add_child(flip, file("w", 1, 0));
        t
    }

    fn by_path<'a>(d: &'a [DiffEntry], path: &str) -> Vec<&'a DiffEntry> {
        d.iter()
            .filter(|e| e.path.eq_ignore_ascii_case(path))
            .collect()
    }

    #[test]
    fn tree_diff_folders() {
        let d = diff_trees(&old_tree(), &new_tree(), &DiffOptions::default());
        let paths: Vec<_> = d.iter().map(|e| (e.path.as_str(), e.status)).collect();
        assert_eq!(
            paths,
            [
                ("", Status::Grown),
                ("docs", Status::Grown),
                ("Fresh", Status::New),
                ("flip", Status::New),
                ("Gone", Status::Deleted),
            ]
        );
        let docs = by_path(&d, "docs")[0];
        assert_eq!(docs.old.allocated, 12288 + 20480);
        assert_eq!(docs.new.allocated, 90112 + 4096);
        assert_eq!(docs.parent, Some(0));
        // The deleted file `flip` is not listed but counts in the root.
        assert_eq!(
            d[0].delta(),
            d.iter().skip(1).map(|e| e.delta()).sum::<i64>() - 4096
        );
    }

    #[test]
    fn tree_diff_files_and_unchanged() {
        let opts = DiffOptions {
            files: true,
            unchanged: true,
            same_within: 0,
        };
        let d = diff_trees(&old_tree(), &new_tree(), &opts);
        let a = by_path(&d, "docs\\a.txt")[0];
        assert_eq!(
            (a.status, a.old.logical, a.new.logical),
            (Status::Grown, 10_000, 90_000)
        );
        assert_eq!(by_path(&d, "docs\\b.txt")[0].status, Status::Deleted);
        assert_eq!(by_path(&d, "docs\\c.txt")[0].status, Status::New);
        assert_eq!(by_path(&d, "Same\\Inner\\y")[0].status, Status::Same);
        let flip: Vec<_> = by_path(&d, "flip")
            .iter()
            .map(|e| (e.dir, e.status))
            .collect();
        assert_eq!(flip, [(false, Status::Deleted), (true, Status::New)]);
        for (i, e) in d.iter().enumerate().skip(1) {
            let p = e.parent.unwrap() as usize;
            assert!(p < i && e.path.starts_with(&d[p].path));
        }
        let lenient = DiffOptions {
            same_within: 1 << 20,
            ..DiffOptions::default()
        };
        let d = diff_trees(&old_tree(), &new_tree(), &lenient);
        assert_eq!(by_path(&d, "docs").len(), 0);
    }

    #[test]
    fn many_children_and_duplicate_names() {
        let build = |grow: u64| {
            let mut t = Tree::new("C:\\");
            for i in 0..40u64 {
                let d = t.add_child(t.root(), dir(if i % 2 == 0 { "dup" } else { "DUP" }));
                t.add_child(d, file("f", 4096 * (i + 1) + grow * u64::from(i == 39), 0));
            }
            t
        };
        let d = diff_trees(&build(0), &build(0), &DiffOptions::default());
        assert_eq!(d.len(), 1);
        let d = diff_trees(&build(0), &build(1), &DiffOptions::default());
        assert_eq!(d.len(), 2);
        assert_eq!((d[1].status, d[1].delta()), (Status::Grown, 4096));
    }

    #[test]
    fn movers() {
        let opts = DiffOptions {
            files: true,
            ..DiffOptions::default()
        };
        let d = diff_trees(&old_tree(), &new_tree(), &opts);
        let names = |v: Vec<usize>| v.into_iter().map(|i| d[i].path.clone()).collect::<Vec<_>>();
        assert_eq!(names(top_growers(&d, 2, false)), ["Fresh", "docs\\A.TXT"]);
        assert_eq!(
            names(top_growers(&d, 10, true)),
            ["Fresh", "docs\\A.TXT", "docs\\c.txt", "flip"]
        );
        assert_eq!(
            names(top_shrinkers(&d, 10, true)),
            ["Gone", "docs\\b.txt", "flip"]
        );
    }

    #[test]
    fn summary_diff() {
        let opts = SummaryOptions {
            min_fraction: 0.5,
            max_depth: 1,
        };
        let mut old = old_tree();
        let deep = old.find_path("Same\\Inner").unwrap();
        old.add_child(deep, file("big", 1 << 20, 0));
        let a = Summary::from_tree(&old, 0, opts);
        let b = Summary::from_tree(&new_tree(), 1, opts);
        let d = diff_summaries(&a, &b, &DiffOptions::default());
        let got: Vec<_> = d
            .iter()
            .map(|e| (e.path.as_str(), e.status, e.approx))
            .collect();
        assert_eq!(
            got,
            [
                ("", Status::Grown, false),
                ("docs", Status::Grown, false),
                ("Fresh", Status::New, false),
                ("Same", Status::Shrunk, false),
                ("flip", Status::New, false),
                ("Gone", Status::Deleted, false),
                ("Same\\Inner", Status::Shrunk, true),
            ]
        );
        assert_eq!(d[6].parent, Some(3));
        assert_eq!(
            names_of(&d, top_shrinkers(&d, 5, true)),
            ["Same\\Inner", "Gone"]
        );
    }

    fn names_of(d: &[DiffEntry], v: Vec<usize>) -> Vec<&str> {
        v.into_iter().map(|i| d[i].path.as_str()).collect()
    }
}
