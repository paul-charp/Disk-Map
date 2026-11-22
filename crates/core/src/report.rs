//! Change report between two timeline points: totals and the top movers.

use crate::diff::{DiffOptions, diff_summaries, top_growers, top_shrinkers};
use crate::summary::Summary;

const DAY: i64 = 86_400;

/// A folder that changed, with its allocated size change in bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mover {
    /// Path relative to the root.
    pub path: String,
    pub delta: i64,
}

/// Totals and top movers between two points.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Report {
    /// Allocated bytes of the root in the old point.
    pub old_total: u64,
    /// Allocated bytes of the root in the new point.
    pub new_total: u64,
    pub growers: Vec<Mover>,
    pub shrinkers: Vec<Mover>,
}

impl Report {
    /// Allocated size change of the root.
    pub fn delta(&self) -> i64 {
        (i128::from(self.new_total) - i128::from(self.old_total))
            .clamp(i64::MIN.into(), i64::MAX.into()) as i64
    }

    /// The mover with the largest change either way, growers first on a tie.
    pub fn top(&self) -> Option<&Mover> {
        self.growers.iter().chain(&self.shrinkers).reduce(|a, b| {
            if b.delta.unsigned_abs() > a.delta.unsigned_abs() {
                b
            } else {
                a
            }
        })
    }
}

/// Compares two points, listing up to `n` growers and shrinkers without counting a change twice.
pub fn report(old: &Summary, new: &Summary, n: usize) -> Report {
    let entries = diff_summaries(old, new, &DiffOptions::default());
    let pick = |idx: Vec<usize>| {
        idx.into_iter()
            .map(|i| Mover {
                path: entries[i].path.clone(),
                delta: entries[i].delta(),
            })
            .collect()
    };
    Report {
        old_total: old.root().allocated,
        new_total: new.root().allocated,
        growers: pick(top_growers(&entries, n, true)),
        shrinkers: pick(top_shrinkers(&entries, n, true)),
    }
}

/// The point to compare a new one taken at `now` with: the newest from an earlier UTC day,
/// else the newest. `times` are Unix seconds.
pub fn previous(times: &[i64], now: i64) -> Option<usize> {
    let today = now.div_euclid(DAY);
    let newest = |earlier: bool| {
        (0..times.len())
            .filter(|&i| !earlier || times[i].div_euclid(DAY) < today)
            .max_by_key(|&i| times[i])
    };
    newest(true).or_else(|| newest(false))
}

/// Start of a window ending at the newest point: the newest earlier point at least `days` old
/// (half a day of slack), or the oldest when `days` is `None`. `times` are Unix seconds, oldest first.
pub fn window_start(times: &[i64], days: Option<i64>) -> Option<usize> {
    let last = times.len().checked_sub(1).filter(|&i| i > 0)?;
    let Some(days) = days else {
        return Some(0);
    };
    let limit = times[last] - days * DAY + DAY / 2;
    (0..last).rev().find(|&i| times[i] <= limit)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::summary::{SummaryEntry, SummaryOptions};

    #[test]
    fn window_starts() {
        let times = [0, 3 * DAY, 6 * DAY + 3600, 10 * DAY];
        assert_eq!(window_start(&times, Some(7)), Some(1));
        assert_eq!(window_start(&times, Some(4)), Some(2));
        assert_eq!(window_start(&times, Some(30)), None);
        assert_eq!(window_start(&times, None), Some(0));
        assert_eq!(window_start(&[5], None), None);
        assert_eq!(window_start(&[], Some(7)), None);
    }

    fn point(sizes: &[(&str, u64)]) -> Summary {
        let total = sizes.iter().map(|s| s.1).sum();
        let mut entries = vec![SummaryEntry {
            path: String::new(),
            parent: None,
            depth: 0,
            allocated: total,
            logical: total,
            files: 0,
            dirs: sizes.len() as u64,
        }];
        entries.extend(sizes.iter().map(|&(p, s)| SummaryEntry {
            path: p.into(),
            parent: Some(0),
            depth: 1,
            allocated: s,
            logical: s,
            files: 1,
            dirs: 0,
        }));
        Summary {
            root_path: r"C:\".into(),
            scan_time: 0,
            options: SummaryOptions::default(),
            entries,
        }
    }

    #[test]
    fn totals_and_movers() {
        let old = point(&[("a", 100), ("b", 500), ("c", 50)]);
        let new = point(&[("a", 400), ("b", 200), ("c", 60)]);
        let r = report(&old, &new, 5);
        assert_eq!((r.old_total, r.new_total, r.delta()), (650, 660, 10));
        assert_eq!(
            r.growers[0],
            Mover {
                path: "a".into(),
                delta: 300
            }
        );
        assert_eq!(r.growers.len(), 2);
        assert_eq!(
            r.shrinkers,
            [Mover {
                path: "b".into(),
                delta: -300
            }]
        );
        assert_eq!(r.top().map(|m| m.path.as_str()), Some("a"));
        assert_eq!(report(&old, &new, 1).growers.len(), 1);
    }

    #[test]
    fn previous_prefers_an_earlier_day() {
        let now = 10 * DAY + 3600 * 5;
        let times = [9 * DAY + 100, 10 * DAY + 60, 8 * DAY];
        assert_eq!(previous(&times, now), Some(0));
        assert_eq!(previous(&[10 * DAY + 60], now), Some(0));
        assert_eq!(previous(&[], now), None);
    }
}
