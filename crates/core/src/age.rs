//! File age buckets, used to colour files and folders by modification time.

use crate::layout::Sizes;
use crate::tree::{NodeId, NodeKind, Tree};

/// FILETIME ticks in a day.
pub const TICKS_PER_DAY: i64 = 864_000_000_000;

/// Upper bound in days of every bucket but the last: a week, a month, six months, a year, three years.
const LIMIT_DAYS: [i64; 5] = [7, 30, 182, 365, 1095];

/// Number of age buckets; the last holds everything older, and unknown times.
pub const AGE_BUCKETS: usize = LIMIT_DAYS.len() + 1;

/// Bucket of a modification time relative to `now`, both Windows FILETIME ticks; 0 is the most recent.
pub fn age_bucket(mtime: i64, now: i64) -> u8 {
    if mtime <= 0 {
        return (AGE_BUCKETS - 1) as u8;
    }
    let age = now.saturating_sub(mtime);
    LIMIT_DAYS
        .iter()
        .position(|&d| age < d * TICKS_PER_DAY)
        .unwrap_or(AGE_BUCKETS - 1) as u8
}

/// Age bucket of every node, indexed by node id. A folder takes the bucket where the bytes of its
/// files, counted from the most recent bucket, first reach half its total; empty folders and
/// detached nodes take the last bucket.
pub fn folder_buckets(tree: &Tree, sizes: Sizes<'_>, now: i64) -> Vec<u8> {
    let n = tree.len();
    let mut hist = vec![[0u64; AGE_BUCKETS]; n];
    let mut out = vec![(AGE_BUCKETS - 1) as u8; n];
    // Children always have larger ids than their parent, so one reverse pass is bottom-up.
    for i in (0..n).rev() {
        let id = NodeId(i as u32);
        if tree.get(id).kind == NodeKind::File {
            let b = age_bucket(tree.times(id).mtime, now);
            out[i] = b;
            hist[i][usize::from(b)] = sizes.get(tree, id);
        } else {
            out[i] = median_bucket(&hist[i]);
        }
        if let Some(p) = tree.parent(id) {
            let h = hist[i];
            for (to, v) in hist[p.0 as usize].iter_mut().zip(h) {
                *to += v;
            }
        }
    }
    out
}

fn median_bucket(hist: &[u64; AGE_BUCKETS]) -> u8 {
    let total: u64 = hist.iter().sum();
    let mut acc = 0;
    for (b, &v) in hist.iter().enumerate() {
        acc += v;
        if total > 0 && acc * 2 >= total {
            return b as u8;
        }
    }
    (AGE_BUCKETS - 1) as u8
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::{NewEntry, Times};

    const NOW: i64 = 134_000_000_000_000_000;

    fn days_ago(d: i64) -> u8 {
        age_bucket(NOW - d * TICKS_PER_DAY, NOW)
    }

    #[test]
    fn buckets_by_age() {
        assert_eq!(days_ago(0), 0);
        assert_eq!(days_ago(6), 0);
        assert_eq!(days_ago(7), 1);
        assert_eq!(days_ago(29), 1);
        assert_eq!(days_ago(100), 2);
        assert_eq!(days_ago(300), 3);
        assert_eq!(days_ago(1000), 4);
        assert_eq!(days_ago(1095), 5);
        assert_eq!(days_ago(10_000), 5);
    }

    #[test]
    fn future_and_unknown_times() {
        assert_eq!(days_ago(-3), 0);
        assert_eq!(age_bucket(0, NOW), (AGE_BUCKETS - 1) as u8);
    }

    fn add(
        tree: &mut Tree,
        parent: NodeId,
        name: &str,
        kind: NodeKind,
        size: u64,
        days: i64,
    ) -> NodeId {
        let mtime = NOW - days * TICKS_PER_DAY;
        let entry = NewEntry {
            name,
            kind,
            attrs: 0,
            logical: size,
            allocated: size,
            times: Times {
                mtime,
                ..Times::default()
            },
        };
        tree.add_child(parent, entry)
    }

    #[test]
    fn folder_median_by_size() {
        let mut t = Tree::new("r");
        let root = t.root();
        let old = add(&mut t, root, "old", NodeKind::Dir, 0, 0);
        add(&mut t, old, "big", NodeKind::File, 1000, 2000);
        for i in 0..4 {
            add(&mut t, old, &format!("n{i}"), NodeKind::File, 10, 1);
        }
        let new = add(&mut t, root, "new", NodeKind::Dir, 0, 0);
        add(&mut t, new, "big", NodeKind::File, 1000, 1);
        add(&mut t, new, "small", NodeKind::File, 10, 2000);
        add(&mut t, root, "empty", NodeKind::Dir, 0, 0);
        let b = folder_buckets(&t, Sizes::Allocated, NOW);
        assert_eq!(b[old.0 as usize], 5);
        assert_eq!(b[new.0 as usize], 0);
        let empty = t.find_child(root, "empty").expect("empty");
        assert_eq!(b[empty.0 as usize], 5);
    }

    #[test]
    fn folder_buckets_propagate_up() {
        let mut t = Tree::new("r");
        let root = t.root();
        let a = add(&mut t, root, "a", NodeKind::Dir, 0, 0);
        let c = add(&mut t, a, "c", NodeKind::Dir, 0, 0);
        let f = add(&mut t, c, "f", NodeKind::File, 500, 100);
        add(&mut t, root, "g", NodeKind::File, 100, 1);
        let b = folder_buckets(&t, Sizes::Allocated, NOW);
        assert_eq!(b[f.0 as usize], 2);
        assert_eq!(b[c.0 as usize], 2);
        assert_eq!(b[a.0 as usize], 2);
        assert_eq!(b[root.0 as usize], 2);
    }
}
