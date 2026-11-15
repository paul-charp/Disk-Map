//! Headless snapshot run: scan each root, write a timeline point, report and exit without a window.

use crate::args::Headless;
use crate::config::Config;
use crate::scan::Scan;
use crate::state::log_done;
use crate::timeline::{self, ReportLine};
use crate::{fmt, log, toast};
use diskmap_core::report;
use diskmap_core::retention::unix_from_filetime;
use std::path::Path;

const TOP: usize = 5;

/// Runs the snapshots; returns the process exit code (0 when every root succeeded).
pub fn run(opts: &Headless, cfg: &Config) -> i32 {
    let mut lines = Vec::new();
    let mut failed = 0;
    for root in &opts.roots {
        match snapshot(root, cfg) {
            Ok(line) => lines.push(line),
            Err(e) => {
                log::error("snapshot", format!("{root}: {e}"));
                failed += 1;
            }
        }
    }
    if !opts.quiet {
        for l in &lines {
            println!("{}", summary_line(l));
        }
    }
    if !opts.no_toast && !lines.is_empty() {
        let (title, body) = toast_text(&lines);
        toast::show(&title, &body);
    }
    i32::from(failed > 0)
}

fn scan(root: &str, cfg: &Config) -> Result<Scan, String> {
    let path = Path::new(root.trim());
    if !path.is_dir() {
        return Err("Not a folder".into());
    }
    let (mut scan, rx) = Scan::start(path, cfg.scan_threads(), cfg.backend_pref());
    loop {
        let Ok(event) = rx.recv() else {
            scan.finish();
            break;
        };
        if scan.apply(event) {
            break;
        }
    }
    log_done(&scan);
    if scan.completed() {
        Ok(scan)
    } else {
        Err("Scan failed".into())
    }
}

/// Scans one root, stores its point, applies retention and appends the report line.
fn snapshot(root: &str, cfg: &Config) -> Result<ReportLine, String> {
    let scan = scan(root, cfg)?;
    let point = timeline::point_of(scan.tree(), fmt::filetime_now());
    drop(scan);
    let now = unix_from_filetime(point.scan_time);
    let points = timeline::list(&point.root_path);
    let times: Vec<i64> = points.iter().map(|p| p.time).collect();
    let previous = report::previous(&times, now).and_then(|i| {
        let p = &points[i];
        timeline::load(&p.path)
            .map_err(|e| log::warn("snapshot", e))
            .ok()
            .map(|old| (p.time, report::report(&old, &point, TOP)))
    });
    timeline::write(&point)?;
    timeline::prune(&point.root_path, now);
    let line = ReportLine::new(&point, previous);
    if let Err(e) = timeline::append_report(&line) {
        log::warn("snapshot", e);
    }
    log::info("snapshot", summary_line(&line));
    Ok(line)
}

/// `C:` for a drive root, the path otherwise.
fn label(root: &str) -> &str {
    match root.strip_suffix('\\') {
        Some(drive) if drive.ends_with(':') => drive,
        _ => root,
    }
}

fn full_path(root: &str, rel: &str) -> String {
    if root.ends_with('\\') {
        format!("{root}{rel}")
    } else {
        format!("{root}\\{rel}")
    }
}

fn delta(line: &ReportLine) -> Option<i64> {
    line.old
        .map(|old| (i128::from(line.new) - i128::from(old)) as i64)
}

/// `C: +4.2 GB`, or `C: 412 GB` for a first point.
fn summary_line(line: &ReportLine) -> String {
    let change = delta(line).map_or_else(|| fmt::size(line.new), fmt::signed_size);
    format!("{} {change}", label(&line.root))
}

/// Title with every root's change, body with the largest single move.
fn toast_text(lines: &[ReportLine]) -> (String, String) {
    let title = lines
        .iter()
        .map(summary_line)
        .collect::<Vec<_>>()
        .join(" · ");
    let top = lines
        .iter()
        .flat_map(|l| l.growers.iter().chain(&l.shrinkers).map(move |m| (l, m)))
        .max_by_key(|(_, m)| m.delta.unsigned_abs());
    let body = match top {
        Some((l, m)) => format!(
            "{} {}",
            full_path(&l.root, &m.path),
            fmt::signed_size(m.delta)
        ),
        None if lines.iter().all(|l| l.prev.is_none()) => "First snapshot".into(),
        None => "No change".into(),
    };
    (title, body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::timeline::Mover;

    fn line(root: &str, old: Option<u64>, new: u64, movers: &[(&str, i64)]) -> ReportLine {
        ReportLine {
            time: 0,
            root: root.into(),
            prev: old.map(|_| 0),
            old,
            new,
            growers: movers
                .iter()
                .map(|&(p, d)| Mover {
                    path: p.into(),
                    delta: d,
                })
                .collect(),
            shrinkers: Vec::new(),
        }
    }

    #[test]
    fn toast_lines() {
        let gb = 1 << 30;
        let lines = [
            line(r"C:\", Some(10 * gb), 14 * gb, &[(r"Users\me", 3 << 30)]),
            line(r"D:\data", None, gb, &[]),
        ];
        let (title, body) = toast_text(&lines);
        assert_eq!(title, r"C: +4 GB · D:\data 1 GB");
        assert_eq!(body, r"C:\Users\me +3 GB");
        let (_, body) = toast_text(&lines[1..]);
        assert_eq!(body, "First snapshot");
    }
}
