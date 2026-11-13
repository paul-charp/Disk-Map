//! Compare mode of a tab: the snapshot tree, the per-node comparison and the top movers.

use diskmap_core::compare::Compare;
use diskmap_core::diff::{DiffEntry, DiffOptions, diff_trees, top_growers, top_shrinkers};
use diskmap_core::tree::Tree;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Least time between two recomputations after live changes.
const REFRESH: Duration = Duration::from_secs(3);
/// Movers kept per direction for the whole tree.
const MOVERS: usize = 500;

/// The snapshot a tab is compared against.
#[derive(Clone)]
pub struct Base {
    pub tree: Arc<Tree>,
    pub path: PathBuf,
    /// Scan time of the snapshot (FILETIME).
    pub time: i64,
}

/// Largest changes, as diff entries in descending order of size change.
#[derive(Default)]
pub struct Movers {
    pub entries: Vec<DiffEntry>,
    pub growers: Vec<usize>,
    pub shrinkers: Vec<usize>,
    tree_ver: Option<u64>,
    at: Option<Instant>,
}

/// Compare mode state.
pub struct CompareState {
    pub base: Base,
    pub map: Compare,
    tree_ver: u64,
    at: Instant,
    movers: Movers,
}

impl CompareState {
    /// Compares `current` against a snapshot.
    pub fn new(base: Base, current: &Tree, tree_ver: u64) -> Self {
        let map = Compare::new(&base.tree, current);
        CompareState {
            base,
            map,
            tree_ver,
            at: Instant::now(),
            movers: Movers::default(),
        }
    }

    /// Recomputes the comparison when the tree changed, at most every few seconds unless `now`.
    /// Returns true when it did.
    pub fn refresh(&mut self, current: &Tree, tree_ver: u64, now: bool) -> bool {
        if tree_ver == self.tree_ver || (!now && self.at.elapsed() < REFRESH) {
            return false;
        }
        self.map = Compare::new(&self.base.tree, current);
        self.tree_ver = tree_ver;
        self.at = Instant::now();
        true
    }

    /// Top growers and shrinkers of the whole tree, recomputed like the comparison.
    pub fn movers(&mut self, current: &Tree, tree_ver: u64) -> &Movers {
        let m = &self.movers;
        let fresh = m.tree_ver == Some(tree_ver)
            || (m.tree_ver.is_some() && m.at.is_some_and(|t| t.elapsed() < REFRESH));
        if !fresh {
            self.movers = movers(&self.base.tree, current, tree_ver);
        }
        &self.movers
    }
}

fn movers(old: &Tree, new: &Tree, tree_ver: u64) -> Movers {
    let options = DiffOptions {
        same_within: 0,
        files: true,
        unchanged: false,
    };
    let entries = diff_trees(old, new, &options);
    Movers {
        growers: top_growers(&entries, MOVERS, true),
        shrinkers: top_shrinkers(&entries, MOVERS, true),
        entries,
        tree_ver: Some(tree_ver),
        at: Some(Instant::now()),
    }
}

/// True when a path relative to the scan root lies at or below `root` (also relative).
pub fn under(path: &str, root: &str) -> bool {
    if root.is_empty() {
        return true;
    }
    let Some(head) = path.get(..root.len()) else {
        return false;
    };
    let rest = &path[root.len()..];
    head.eq_ignore_ascii_case(root) && (rest.is_empty() || rest.starts_with('\\'))
}

#[cfg(test)]
mod tests {
    use super::under;

    #[test]
    fn paths_below_a_root() {
        assert!(under(r"a\b", ""));
        assert!(under(r"A\b", "a"));
        assert!(under("a", "a"));
        assert!(!under("ab", "a"));
    }
}
