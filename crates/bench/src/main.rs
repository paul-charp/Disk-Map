//! `diskmap-bench`: scan benchmarks (`scan`, `compare`) and a live change printer (`watch`).

use crossbeam_channel::{RecvTimeoutError, unbounded};
use diskmap_core::tree::{Node, NodeKind, Tree};
use diskmap_scan::volume::volume_info;
use diskmap_scan::walk::{absolute_root, default_threads};
use diskmap_scan::{
    Backend, ChangeKind, LiveUpdater, MftScanner, MftStats, MftSupport, ScanError, Scanner,
    TreeBuilder, WalkScanner, WatchKind, Watcher, auto_scanner, mft_available,
};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

const GIB: f64 = (1u64 << 30) as f64;
const MIB: f64 = (1u64 << 20) as f64;

const USAGE: &str = "usage:
  diskmap-bench scan <path> [--backend walk|mft|auto] [--threads N]
  diskmap-bench compare <path> [--threads N]
  diskmap-bench watch <path> [--for SECONDS]";

fn usage() -> ExitCode {
    eprintln!("{USAGE}");
    ExitCode::from(2)
}

struct Args {
    path: PathBuf,
    backend: String,
    threads: Option<usize>,
    seconds: Option<u64>,
}

fn parse(mut it: impl Iterator<Item = String>) -> Option<Args> {
    let mut args = Args {
        path: PathBuf::new(),
        backend: "walk".into(),
        threads: None,
        seconds: None,
    };
    let mut path = None;
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--threads" => args.threads = Some(it.next()?.parse().ok().filter(|&n| n > 0)?),
            "--backend" => {
                args.backend = it.next()?;
                if !["walk", "mft", "auto"].contains(&args.backend.as_str()) {
                    return None;
                }
            }
            "--for" => args.seconds = Some(it.next()?.parse().ok()?),
            _ if path.is_none() && !arg.starts_with("--") => path = Some(PathBuf::from(arg)),
            _ => return None,
        }
    }
    args.path = path?;
    Some(args)
}

fn main() -> ExitCode {
    let mut it = std::env::args().skip(1);
    let Some(command) = it.next() else {
        return usage();
    };
    let Some(args) = parse(it) else {
        return usage();
    };
    match command.as_str() {
        "scan" => cmd_scan(&args),
        "compare" => cmd_compare(&args),
        "watch" => cmd_watch(&args),
        _ => usage(),
    }
}

struct Outcome {
    tree: Tree,
    errors: Vec<ScanError>,
    secs: f64,
    backend: Backend,
    mft: Option<Arc<MftStats>>,
}

fn scan_with(path: &Path, backend: Backend, threads: Option<usize>) -> Outcome {
    let root = absolute_root(path);
    let (tx, rx) = unbounded();
    let mut builder = TreeBuilder::new(&root);
    let mut mft = None;
    let scanner: Box<dyn Scanner> = match backend {
        Backend::Walk => Box::new(WalkScanner::new(threads)),
        Backend::Mft => {
            let s = MftScanner::new(threads);
            mft = Some(s.stats());
            Box::new(s)
        }
    };
    let start = Instant::now();
    scanner.start(path.to_path_buf(), tx, Arc::new(AtomicBool::new(false)));
    let result = builder.drain(&rx);
    Outcome {
        tree: builder.into_tree(),
        errors: result.errors,
        secs: start.elapsed().as_secs_f64(),
        backend,
        mft,
    }
}

fn explain_unavailable(support: MftSupport) {
    eprintln!("MFT backend unavailable: {}.", support.reason());
    if support == MftSupport::NotElevated {
        eprintln!("Run this command from an administrator terminal.");
    }
}

fn cmd_scan(args: &Args) -> ExitCode {
    let backend = match args.backend.as_str() {
        "walk" => Backend::Walk,
        "mft" => {
            let support = mft_available(&args.path);
            if support != MftSupport::Available {
                explain_unavailable(support);
                return ExitCode::from(1);
            }
            Backend::Mft
        }
        _ => auto_scanner(&args.path, args.threads).1,
    };
    let out = scan_with(&args.path, backend, args.threads);
    print_scan(&args.path, args, &out);
    ExitCode::SUCCESS
}

fn print_scan(path: &Path, args: &Args, out: &Outcome) {
    let tree = &out.tree;
    let files = tree.file_count();
    let node = tree.get(tree.root());
    let mem = tree.len() * std::mem::size_of::<Node>() + tree.names_bytes();

    println!("path:             {}", absolute_root(path));
    println!("backend:          {}", out.backend);
    println!(
        "threads:          {}",
        args.threads.unwrap_or_else(default_threads)
    );
    println!("elapsed:          {:.3} s", out.secs);
    println!("files:            {files}");
    println!("directories:      {}", tree.dir_count());
    println!("files/second:     {:.0}", files as f64 / out.secs);
    println!(
        "logical bytes:    {} ({:.2} GiB)",
        node.logical,
        node.logical as f64 / GIB
    );
    println!(
        "allocated bytes:  {} ({:.2} GiB)",
        node.allocated,
        node.allocated as f64 / GIB
    );
    if let Some(m) = &out.mft {
        let bytes = m.bytes_read.load(Ordering::Relaxed);
        println!(
            "mft records read: {} ({} in use, {} bad)",
            m.records_read.load(Ordering::Relaxed),
            m.records_in_use.load(Ordering::Relaxed),
            m.records_bad.load(Ordering::Relaxed)
        );
        println!(
            "mft bytes read:   {} ({:.1} MiB)",
            bytes,
            bytes as f64 / MIB
        );
    }
    println!("errors:           {}", out.errors.len());
    for e in out.errors.iter().take(10) {
        println!("  error {}: {}", e.code, e.path);
    }
    println!(
        "tree memory:      {} bytes ({:.1} MiB)",
        mem,
        mem as f64 / MIB
    );

    match volume_info(path) {
        Ok(v) if v.is_volume_root => {
            let used = v.total_bytes.saturating_sub(v.free_bytes);
            println!(
                "volume used:      {} ({:.2} GiB), cluster size {}",
                used,
                used as f64 / GIB,
                v.cluster_size
            );
            let diff = used as i128 - node.allocated as i128;
            println!("used - allocated: {} ({:.2} GiB)", diff, diff as f64 / GIB);
        }
        Ok(_) => {}
        Err(e) => println!("volume info:      unavailable ({e})"),
    }
}

fn cmd_compare(args: &Args) -> ExitCode {
    let support = mft_available(&args.path);
    if support != MftSupport::Available {
        explain_unavailable(support);
        return ExitCode::from(1);
    }
    let walk = scan_with(&args.path, Backend::Walk, args.threads);
    let mft = scan_with(&args.path, Backend::Mft, args.threads);

    println!("path:    {}", absolute_root(&args.path));
    let row = |name: &str, f: &dyn Fn(&Outcome) -> i128| {
        let (w, m) = (f(&walk), f(&mft));
        println!(
            "{name:<16} walk {w:>18}  mft {m:>18}  diff (mft - walk) {:>+16}",
            m - w
        );
    };
    println!(
        "{:<16} walk {:>17.3}s  mft {:>17.3}s",
        "elapsed", walk.secs, mft.secs
    );
    row("files", &|o| o.tree.file_count() as i128);
    row("directories", &|o| o.tree.dir_count() as i128);
    row("logical bytes", &|o| {
        i128::from(o.tree.get(o.tree.root()).logical)
    });
    row("allocated bytes", &|o| {
        i128::from(o.tree.get(o.tree.root()).allocated)
    });
    row("errors", &|o| o.errors.len() as i128);
    if let Some(m) = &mft.mft {
        println!(
            "mft: {} records read ({} in use, {} bad), {:.1} MiB read",
            m.records_read.load(Ordering::Relaxed),
            m.records_in_use.load(Ordering::Relaxed),
            m.records_bad.load(Ordering::Relaxed),
            m.bytes_read.load(Ordering::Relaxed) as f64 / MIB
        );
    }

    let top = |t: &Tree| -> HashMap<String, (String, u64)> {
        t.children(t.root())
            .map(|c| {
                (
                    t.name(c).to_lowercase(),
                    (t.name(c).to_string(), t.get(c).allocated),
                )
            })
            .collect()
    };
    let (w, m) = (top(&walk.tree), top(&mft.tree));
    let mut names: Vec<&String> = w
        .keys()
        .chain(m.keys().filter(|k| !w.contains_key(*k)))
        .collect();
    names.sort();
    let mut rows: Vec<(&str, u64, u64)> = names
        .into_iter()
        .map(|k| {
            let label = w
                .get(k)
                .or_else(|| m.get(k))
                .map_or("", |(n, _)| n.as_str());
            (
                label,
                w.get(k).map_or(0, |v| v.1),
                m.get(k).map_or(0, |v| v.1),
            )
        })
        .collect();
    rows.sort_by_key(|&(_, a, b)| std::cmp::Reverse(a.abs_diff(b)));
    println!("\ntop-level entries with the largest allocated difference:");
    for (name, a, b) in rows.iter().take(10) {
        println!(
            "  {name:<40} walk {a:>16}  mft {b:>16}  diff {:>+16}",
            *b as i128 - *a as i128
        );
    }
    ExitCode::SUCCESS
}

fn cmd_watch(args: &Args) -> ExitCode {
    let root = absolute_root(&args.path);
    let path = PathBuf::from(&root);
    let (scanner, backend) = auto_scanner(&path, args.threads);
    let (tx, rx) = unbounded();
    let mut builder = TreeBuilder::new(&root);
    let start = Instant::now();
    scanner.start(path.clone(), tx, Arc::new(AtomicBool::new(false)));
    builder.drain(&rx);
    let mut tree = builder.into_tree();
    println!(
        "scanned {root} with {backend} in {:.3} s: {} files, {} directories",
        start.elapsed().as_secs_f64(),
        tree.file_count(),
        tree.dir_count()
    );

    let cluster = volume_info(&path).map_or(4096, |v| v.cluster_size);
    let mut updater = LiveUpdater::new(cluster);
    let (wtx, wrx) = unbounded();
    let _watcher = Watcher::start(path.clone(), wtx);
    println!("watching, press Ctrl+C to stop");

    let deadline = args
        .seconds
        .map(|s| Instant::now() + Duration::from_secs(s));
    let mut batch = Vec::new();
    loop {
        if deadline.is_some_and(|d| Instant::now() >= d) {
            break;
        }
        match wrx.recv_timeout(Duration::from_millis(100)) {
            Ok(e) => batch.push(e),
            Err(RecvTimeoutError::Timeout) if !batch.is_empty() => {}
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => break,
        }
        // let a burst settle before applying it
        let settle = Instant::now();
        while settle.elapsed() < Duration::from_secs(1) {
            match wrx.recv_timeout(Duration::from_millis(50)) {
                Ok(e) => batch.push(e),
                Err(_) => break,
            }
        }
        if batch.iter().any(|e| e.kind == WatchKind::Overflow) {
            println!("overflow: changes were lost, rescan needed");
        }
        for c in updater.apply(&mut tree, &path, &batch) {
            let kind = match c.kind {
                ChangeKind::Added => "added  ",
                ChangeKind::Removed => "removed",
                ChangeKind::Grew => "grew   ",
                ChangeKind::Shrank => "shrank ",
            };
            let n = tree.get(c.node);
            let what = if n.kind == NodeKind::File {
                ""
            } else {
                " (dir)"
            };
            println!(
                "{kind} {}{what}  {} bytes ({} allocated)",
                c.path, n.logical, n.allocated
            );
        }
        batch.clear();
    }
    ExitCode::SUCCESS
}
