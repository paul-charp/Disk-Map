//! Timeline points on disk (`<data>/timeline/<root key>/<YYYYMMDD-HHMMSS>.dmtl`, UTC) and the report log.

use crate::{log, paths};
use diskmap_core::key::root_key;
use diskmap_core::report::Report;
use diskmap_core::retention::{keep, parse_stamp, stamp, unix_from_filetime};
use diskmap_core::summary::{Summary, SummaryEntry, SummaryOptions};
use diskmap_core::tree::Tree;
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};

const DIR: &str = "timeline";
const EXT: &str = "dmtl";
const REPORTS: &str = "reports.jsonl";

/// A stored point: its time (Unix seconds, UTC) and file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Point {
    pub time: i64,
    pub path: PathBuf,
}

/// A folder in a report line.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Mover {
    pub path: String,
    pub delta: i64,
}

/// One line of `reports.jsonl`. Times are Unix seconds; sizes are allocated bytes.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReportLine {
    pub time: i64,
    pub root: String,
    /// Time of the point compared with, absent for the first point.
    pub prev: Option<i64>,
    pub old: Option<u64>,
    pub new: u64,
    pub growers: Vec<Mover>,
    pub shrinkers: Vec<Mover>,
}

impl ReportLine {
    /// The line for a new point and, when there was one, its report against the previous point.
    pub fn new(point: &Summary, previous: Option<(i64, Report)>) -> Self {
        let movers = |list: &[diskmap_core::report::Mover]| {
            list.iter()
                .map(|m| Mover {
                    path: m.path.clone(),
                    delta: m.delta,
                })
                .collect()
        };
        let (prev, old, growers, shrinkers) = match &previous {
            Some((t, r)) => (
                Some(*t),
                Some(r.old_total),
                movers(&r.growers),
                movers(&r.shrinkers),
            ),
            None => (None, None, Vec::new(), Vec::new()),
        };
        ReportLine {
            time: unix_from_filetime(point.scan_time),
            root: point.root_path.clone(),
            prev,
            old,
            new: point.root().allocated,
            growers,
            shrinkers,
        }
    }
}

fn root_dir(root: &str) -> Option<PathBuf> {
    Some(paths::data_dir()?.join(DIR).join(root_key(root)))
}

/// Points in a folder, oldest first. Files with other names are ignored.
fn list_in(dir: &Path) -> Vec<Point> {
    let Ok(read) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<Point> = read
        .filter_map(Result::ok)
        .filter_map(|e| {
            let path = e.path();
            if path.extension()? != EXT {
                return None;
            }
            let time = parse_stamp(path.file_stem()?.to_str()?)?;
            Some(Point { time, path })
        })
        .collect();
    out.sort_by_key(|p| p.time);
    out
}

/// Points of a root, oldest first.
pub fn list(root: &str) -> Vec<Point> {
    root_dir(root).map_or_else(Vec::new, |d| list_in(&d))
}

/// Reads and decodes a point.
pub fn load(path: &Path) -> Result<Summary, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    Summary::decode(&bytes).map_err(|e| format!("{}: {e}", path.display()))
}

/// Bytes read for the root totals of a point; the header and root entry fit unless the root path is very long.
const HEAD: u64 = 4096;

/// Reads the root totals of a point from the start of its file.
pub fn load_root(path: &Path) -> Result<SummaryEntry, String> {
    use std::io::Read;
    let err = |e: &dyn std::fmt::Display| format!("{}: {e}", path.display());
    let mut head = Vec::new();
    std::fs::File::open(path)
        .and_then(|f| f.take(HEAD).read_to_end(&mut head))
        .map_err(|e| err(&e))?;
    match Summary::read_root(&head) {
        Ok((_, root)) => Ok(root),
        Err(_) if head.len() as u64 == HEAD => Ok(load(path)?.root().clone()),
        Err(e) => Err(err(&e)),
    }
}

/// Writes a point through a temporary file and a rename.
fn write_in(dir: &Path, point: &Summary) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let name = stamp(unix_from_filetime(point.scan_time));
    let path = dir.join(format!("{name}.{EXT}"));
    let tmp = dir.join(format!("{name}.{EXT}.tmp"));
    std::fs::write(&tmp, point.encode())
        .and_then(|()| std::fs::rename(&tmp, &path))
        .map_err(|e| {
            let _ = std::fs::remove_file(&tmp);
            format!("{}: {e}", path.display())
        })?;
    Ok(path)
}

/// Writes a point under its root's folder.
pub fn write(point: &Summary) -> Result<PathBuf, String> {
    let dir = root_dir(&point.root_path).ok_or("No data folder")?;
    write_in(&dir, point)
}

/// Deletes the points of a folder that retention does not keep; returns how many.
fn prune_in(dir: &Path, now: i64) -> usize {
    let points = list_in(dir);
    let times: Vec<i64> = points.iter().map(|p| p.time).collect();
    let mut removed = 0;
    for (p, kept) in points.iter().zip(keep(&times, now)) {
        if kept {
            continue;
        }
        match std::fs::remove_file(&p.path) {
            Ok(()) => removed += 1,
            Err(e) => log::warn("timeline", format!("{}: {e}", p.path.display())),
        }
    }
    removed
}

/// Applies retention to a root's points; `now` is Unix seconds.
pub fn prune(root: &str, now: i64) -> usize {
    root_dir(root).map_or(0, |d| prune_in(&d, now))
}

/// Timeline point of a finished scan.
pub fn point_of(tree: &Tree, scan_time: i64) -> Summary {
    Summary::from_tree(tree, scan_time, SummaryOptions::default())
}

/// Writes a point and applies retention, logging failures.
pub fn record(point: &Summary) {
    match write(point) {
        Ok(_) => {
            prune(&point.root_path, unix_from_filetime(point.scan_time));
        }
        Err(e) => log::error("timeline", e),
    }
}

/// Appends a line to `reports.jsonl`.
pub fn append_report(line: &ReportLine) -> Result<(), String> {
    let path = paths::ensure_data_dir()
        .map_err(|e| e.to_string())?
        .join(REPORTS);
    let mut text = serde_json::to_string(line).map_err(|e| e.to_string())?;
    text.push('\n');
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .and_then(|mut f| f.write_all(text.as_bytes()))
        .map_err(|e| format!("{}: {e}", path.display()))
}

/// The last `limit` lines of a report log, newest first; unreadable lines are skipped.
fn reports_in(path: &Path, limit: usize) -> Vec<ReportLine> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    text.lines()
        .rev()
        .filter_map(|l| serde_json::from_str(l).ok())
        .take(limit)
        .collect()
}

/// Recent report lines, newest first.
pub fn reports(limit: usize) -> Vec<ReportLine> {
    paths::data_dir().map_or_else(Vec::new, |d| reports_in(&d.join(REPORTS), limit))
}

#[cfg(test)]
mod tests {
    use super::*;
    use diskmap_core::retention::filetime_from_unix;
    use diskmap_core::tree::{NewEntry, NodeKind, Times};

    fn point(secs: i64) -> Summary {
        let mut t = Tree::new(r"C:\");
        let root = t.root();
        t.add_child(
            root,
            NewEntry {
                name: "a.bin",
                kind: NodeKind::File,
                attrs: 0,
                logical: 10,
                allocated: 4096,
                times: Times::default(),
            },
        );
        point_of(&t, filetime_from_unix(secs))
    }

    #[test]
    fn write_list_load_and_prune() {
        let dir = tempfile::tempdir().unwrap();
        let day = 86_400;
        let now = 20_000 * day;
        for secs in [now - 3600, now - 7200, now - 400 * day, now - 440 * day] {
            write_in(dir.path(), &point(secs)).unwrap();
        }
        std::fs::write(dir.path().join("notes.txt"), "x").unwrap();
        let points = list_in(dir.path());
        assert_eq!(points.len(), 4);
        assert_eq!(points[0].time, now - 440 * day);
        assert_eq!(load(&points[3].path).unwrap().root().allocated, 4096);
        assert_eq!(prune_in(dir.path(), now), 1);
        assert_eq!(list_in(dir.path()).len(), 3);
    }

    #[test]
    fn report_lines_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(REPORTS);
        let line = ReportLine::new(&point(1000), None);
        let text = serde_json::to_string(&line).unwrap();
        std::fs::write(&path, format!("{text}\nnot json\n{text}\n")).unwrap();
        let read = reports_in(&path, 5);
        assert_eq!(read.len(), 2);
        assert_eq!((read[0].time, read[0].new, read[0].old), (1000, 4096, None));
        assert_eq!(reports_in(&path, 1).len(), 1);
    }
}
