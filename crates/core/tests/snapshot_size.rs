//! Size and speed of the snapshot and timeline formats on a synthetic tree.
//! Run with `cargo test -p diskmap-core --release --test snapshot_size -- --ignored --nocapture`.

use std::time::Instant;

use diskmap_core::diff::{DiffOptions, diff_trees, top_growers};
use diskmap_core::snapshot::{self, SnapshotMeta};
use diskmap_core::summary::{Summary, SummaryOptions};
use diskmap_core::tree::{NewEntry, NodeId, NodeKind, Times, Tree};

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }

    fn pick<'a>(&mut self, items: &[&'a str]) -> &'a str {
        items[self.below(items.len() as u64) as usize]
    }
}

const WORDS: &[&str] = &[
    "Microsoft",
    "Windows",
    "System32",
    "Program Files",
    "node_modules",
    "cache",
    "assets",
    "en-US",
    "Temp",
    "Logs",
    "Packages",
    "Common",
    "Resources",
    "lib",
    "src",
    "build",
    "Debug",
    "Release",
    "Photos",
    "Documents",
    "Steam",
    "WinSxS",
    "drivers",
    "fonts",
    "icons",
    "x64",
    "Data",
    "Config",
];
const EXTS: &[&str] = &[
    "dll", "exe", "txt", "jpg", "png", "json", "js", "mui", "log", "dat", "xml", "cab", "manifest",
];

fn dir_name(rng: &mut Rng) -> String {
    match rng.below(10) {
        0 => format!(
            "{:08x}-{:04x}-{:04x}",
            rng.next() as u32,
            rng.below(65536),
            rng.below(65536)
        ),
        1 => format!(
            "amd64_microsoft-windows-{}_31bf3856ad364e35_10.0.{}.{}_none",
            rng.pick(WORDS).to_lowercase(),
            19041 + rng.below(4000),
            rng.below(5000)
        ),
        2..=4 => format!("{}{}", rng.pick(WORDS), rng.below(100)),
        _ => rng.pick(WORDS).to_owned(),
    }
}

fn file_name(rng: &mut Rng, i: u64) -> String {
    let ext = rng.pick(EXTS);
    match rng.below(8) {
        0 => format!("{:016x}{:016x}.{ext}", rng.next(), rng.next()),
        1 | 2 => format!("IMG_{}.{ext}", 20_000_000 + i),
        3 => format!("{}.{}.{ext}", rng.pick(WORDS), rng.pick(WORDS)),
        _ => format!(
            "{}_{}.{ext}",
            rng.pick(WORDS).to_lowercase(),
            rng.below(1000)
        ),
    }
}

fn synthetic(target: usize) -> Tree {
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    let mut t = Tree::new("C:\\");
    let mut dirs = vec![(t.root(), 0u32, 132_000_000_000_000_000i64)];
    let mut i = 0u64;
    while t.len() < target {
        let (parent, depth, base) = dirs[rng.below(dirs.len() as u64) as usize];
        if rng.below(10) == 0 && depth < 14 {
            let base = base + rng.below(20_000_000_000_000) as i64;
            let id = add(
                &mut t,
                parent,
                &dir_name(&mut rng),
                NodeKind::Dir,
                0,
                base,
                0x10,
            );
            dirs.push((id, depth + 1, base));
            continue;
        }
        let size = match rng.below(10) {
            0 => 0,
            1..=6 => rng.below(64 << 10),
            7 | 8 => rng.below(16 << 20),
            _ => rng.below(2 << 30),
        };
        let spread = if rng.below(2) == 0 {
            10_000_000
        } else {
            1 << 44
        };
        let mtime = base + rng.below(spread) as i64;
        let attrs = if rng.below(20) == 0 { 0x2020 } else { 0x20 };
        let name = file_name(&mut rng, i);
        add(&mut t, parent, &name, NodeKind::File, size, mtime, attrs);
        i += 1;
    }
    t
}

fn add(
    t: &mut Tree,
    parent: NodeId,
    name: &str,
    kind: NodeKind,
    size: u64,
    mtime: i64,
    attrs: u32,
) -> NodeId {
    t.add_child(
        parent,
        NewEntry {
            name,
            kind,
            attrs,
            logical: size,
            allocated: if size < 700 {
                0
            } else {
                size.next_multiple_of(4096)
            },
            times: Times {
                ctime: mtime - 1_000_000_000,
                mtime,
                atime: mtime + 3_000_000_000,
            },
        },
    )
}

#[test]
#[ignore]
fn encoded_sizes() {
    let tree = synthetic(1_000_000);
    let nodes = tree.len() as f64;
    println!(
        "nodes {} (files {}, dirs {}), name bytes per node {:.1}",
        tree.len(),
        tree.file_count(),
        tree.dir_count(),
        tree.names_bytes() as f64 / nodes
    );
    for (label, all_times, seconds) in [
        ("mtime", false, false),
        ("mtime s", false, true),
        ("all times", true, false),
        ("all times s", true, true),
    ] {
        let meta = SnapshotMeta {
            all_times,
            seconds,
            ..SnapshotMeta::default()
        };
        let start = Instant::now();
        let buf = snapshot::encode(&tree, &meta);
        let enc = start.elapsed();
        let start = Instant::now();
        let (back, _) = snapshot::decode(&buf).unwrap();
        let dec = start.elapsed();
        assert_eq!(back.len(), tree.len());
        println!(
            "snapshot {label:12} {:6.2} MB  {:5.2} B/node  encode {:?}  decode {:?}",
            buf.len() as f64 / 1e6,
            buf.len() as f64 / nodes,
            enc,
            dec
        );
    }
    let start = Instant::now();
    let summary = Summary::from_tree(&tree, 0, SummaryOptions::default());
    let made = start.elapsed();
    let buf = summary.encode();
    assert_eq!(Summary::decode(&buf).unwrap(), summary);
    println!(
        "summary entries {}  {:.1} KB  {:.1} B/entry  build {:?}",
        summary.entries.len(),
        buf.len() as f64 / 1e3,
        buf.len() as f64 / summary.entries.len() as f64,
        made
    );
    let (copy, _) = snapshot::decode(&snapshot::encode(&tree, &SnapshotMeta::default())).unwrap();
    let start = Instant::now();
    let d = diff_trees(&tree, &copy, &DiffOptions::default());
    println!(
        "diff of equal trees: {} entries in {:?}",
        d.len(),
        start.elapsed()
    );
    assert_eq!(d.len(), 1);
    assert!(top_growers(&d, 10, true).is_empty());
}
