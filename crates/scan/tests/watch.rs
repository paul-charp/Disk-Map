use crossbeam_channel::{RecvTimeoutError, unbounded};
use diskmap_core::tree::{NodeId, NodeKind, Tree};
use diskmap_scan::volume::volume_info;
use diskmap_scan::walk::absolute_root;
use diskmap_scan::{
    ChangeKind, LiveUpdater, Scanner, TreeBuilder, WalkScanner, WatchEvent, WatchKind, Watcher,
};
use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

fn scan(path: &Path) -> Tree {
    let (tx, rx) = unbounded();
    let mut builder = TreeBuilder::new(&absolute_root(path));
    Box::new(WalkScanner::new(Some(2))).start(
        path.to_path_buf(),
        tx,
        Arc::new(AtomicBool::new(false)),
    );
    let result = builder.drain(&rx);
    assert!(result.errors.is_empty(), "{:?}", result.errors);
    builder.into_tree()
}

fn listing(tree: &Tree) -> BTreeMap<String, (bool, u64, u64)> {
    (1..tree.len() as u32)
        .map(NodeId)
        .filter(|&id| {
            let mut cur = id;
            while let Some(p) = tree.parent(cur) {
                cur = p;
            }
            cur == tree.root()
        })
        .map(|id| {
            let n = tree.get(id);
            (
                tree.full_path(id).to_lowercase(),
                (n.kind == NodeKind::Dir, n.logical, n.allocated),
            )
        })
        .collect()
}

/// Pumps events through the updater until the watcher has been quiet for `quiet`.
fn pump(
    rx: &crossbeam_channel::Receiver<WatchEvent>,
    updater: &mut LiveUpdater,
    tree: &mut Tree,
    root: &Path,
) -> Vec<ChangeKind> {
    let quiet = Duration::from_millis(800);
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut batch = Vec::new();
    loop {
        match rx.recv_timeout(quiet) {
            Ok(e) => {
                assert_ne!(e.kind, WatchKind::Overflow);
                batch.push(e);
            }
            Err(RecvTimeoutError::Timeout) if !batch.is_empty() || Instant::now() > deadline => {
                break;
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
    updater
        .apply(tree, root, &batch)
        .into_iter()
        .map(|c| c.kind)
        .collect()
}

#[test]
fn live_updates_match_a_fresh_scan() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().to_path_buf();
    let cluster = volume_info(&root).unwrap().cluster_size;

    fs::create_dir(root.join("keep")).unwrap();
    fs::write(root.join("keep").join("grow.bin"), vec![1u8; 20_000]).unwrap();
    fs::write(root.join("keep").join("rename_me.bin"), vec![2u8; 30_000]).unwrap();
    fs::write(root.join("delete_me.bin"), vec![3u8; 40_000]).unwrap();
    fs::create_dir(root.join("doomed")).unwrap();
    fs::write(root.join("doomed").join("inside.bin"), vec![4u8; 10_000]).unwrap();

    let mut tree = scan(&root);
    let (tx, rx) = unbounded();
    let watcher = Watcher::start(root.clone(), tx);
    let mut updater = LiveUpdater::new(cluster);

    fs::write(root.join("new.bin"), vec![5u8; 50_000]).unwrap();
    let mut f = fs::OpenOptions::new()
        .append(true)
        .open(root.join("keep").join("grow.bin"))
        .unwrap();
    f.write_all(&vec![6u8; 70_000]).unwrap();
    drop(f);
    fs::rename(
        root.join("keep").join("rename_me.bin"),
        root.join("Renamed.BIN"),
    )
    .unwrap();
    fs::remove_file(root.join("delete_me.bin")).unwrap();
    fs::remove_dir_all(root.join("doomed")).unwrap();
    fs::create_dir_all(root.join("n1").join("n2")).unwrap();
    fs::write(root.join("n1").join("a.bin"), vec![7u8; 9_000]).unwrap();
    fs::write(root.join("n1").join("n2").join("b.bin"), vec![8u8; 12_000]).unwrap();
    fs::write(root.join("blink.bin"), vec![9u8; 5_000]).unwrap();
    fs::remove_file(root.join("blink.bin")).unwrap();

    let kinds = pump(&rx, &mut updater, &mut tree, &root);
    assert!(kinds.contains(&ChangeKind::Added));
    assert!(kinds.contains(&ChangeKind::Removed));
    assert!(kinds.contains(&ChangeKind::Grew));

    // second burst: shrink a file and add into the directory created above
    fs::write(root.join("new.bin"), vec![5u8; 10_000]).unwrap();
    fs::write(root.join("n1").join("n2").join("c.bin"), vec![1u8; 33_000]).unwrap();
    let kinds = pump(&rx, &mut updater, &mut tree, &root);
    assert!(kinds.contains(&ChangeKind::Shrank), "{kinds:?}");

    drop(watcher);
    let fresh = scan(&root);
    let (live, truth) = (listing(&tree), listing(&fresh));
    assert_eq!(live, truth);
    let (a, b) = (tree.get(tree.root()), fresh.get(fresh.root()));
    assert_eq!((a.logical, a.allocated), (b.logical, b.allocated));
    assert_eq!(
        (tree.file_count(), tree.dir_count()),
        (fresh.file_count(), fresh.dir_count())
    );
}

#[test]
fn watcher_stops_promptly_on_drop() {
    let tmp = tempfile::tempdir().unwrap();
    let (tx, _rx) = unbounded();
    let started = Instant::now();
    drop(Watcher::start(tmp.path().to_path_buf(), tx));
    assert!(started.elapsed() < Duration::from_secs(2));
}

#[test]
fn missing_root_reports_overflow() {
    let tmp = tempfile::tempdir().unwrap();
    let (tx, rx) = unbounded();
    let _w = Watcher::start(tmp.path().join("nope"), tx);
    let e = rx.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(e.kind, WatchKind::Overflow);
}

#[test]
fn burst_of_thousands_of_events_is_cheap() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().to_path_buf();
    let cluster = volume_info(&root).unwrap().cluster_size;
    fs::create_dir(root.join("bulk")).unwrap();
    let mut tree = scan(&root);
    let (tx, rx) = unbounded();
    let watcher = Watcher::start(root.clone(), tx);
    let mut updater = LiveUpdater::new(cluster);

    for i in 0..3000 {
        fs::write(
            root.join("bulk").join(format!("f{i:04}.bin")),
            vec![1u8; 4_000 + i],
        )
        .unwrap();
    }
    let mut events = Vec::new();
    let quiet_since = Instant::now();
    while let Ok(e) = rx.recv_timeout(Duration::from_millis(800)) {
        events.push(e);
    }
    assert!(
        events.len() >= 3000,
        "{} events after {:?}",
        events.len(),
        quiet_since.elapsed()
    );
    let start = Instant::now();
    let changes = updater.apply(&mut tree, &root, &events);
    let took = start.elapsed();
    assert_eq!(
        changes
            .iter()
            .filter(|c| c.kind == ChangeKind::Added)
            .count(),
        3000
    );
    assert!(took < Duration::from_secs(5), "{took:?}");
    drop(watcher);
    let fresh = scan(&root);
    assert_eq!(listing(&tree), listing(&fresh));
}
