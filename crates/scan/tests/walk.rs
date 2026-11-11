use crossbeam_channel::unbounded;
use diskmap_core::tree::NodeId;
use diskmap_scan::walk::absolute_root;
use diskmap_scan::{Scanner, TreeBuilder, WalkScanner};
use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

fn find(tree: &diskmap_core::tree::Tree, path: &str) -> Option<NodeId> {
    (0..tree.len() as u32)
        .map(NodeId)
        .find(|&id| tree.full_path(id) == path)
}

#[test]
fn scans_known_tree() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().to_path_buf();

    fs::create_dir(root.join("src")).unwrap();
    fs::write(root.join("src").join("main.rs"), vec![1u8; 1000]).unwrap();
    fs::write(root.join("zero.bin"), b"").unwrap();
    fs::create_dir(root.join("empty")).unwrap();
    fs::write(root.join("caf\u{e9}_\u{65e5}\u{672c}.txt"), vec![2u8; 300]).unwrap();

    let mut long = root.clone();
    for _ in 0..15 {
        long.push("long_directory_name");
    }
    let long_ext = Path::new(r"\\?\").join(&long);
    fs::create_dir_all(&long_ext).unwrap();
    fs::write(long_ext.join("deep.dat"), vec![3u8; 5000]).unwrap();
    assert!(long.to_string_lossy().len() > 260);

    let root_str = absolute_root(&root);
    let (tx, rx) = unbounded();
    Box::new(WalkScanner::new(Some(4))).start(root.clone(), tx, Arc::new(AtomicBool::new(false)));
    let mut builder = TreeBuilder::new(&root_str);
    let result = builder.drain(&rx);
    assert!(result.errors.is_empty(), "{:?}", result.errors);
    assert!(!result.cancelled);

    let tree = builder.tree();
    assert_eq!(tree.file_count(), 4);
    assert_eq!(tree.dir_count(), 2 + 15);
    assert_eq!(tree.get(tree.root()).logical, 1000 + 300 + 5000);

    let main = find(tree, &format!(r"{root_str}\src\main.rs")).expect("main.rs");
    assert_eq!(tree.get(main).logical, 1000);
    assert!(tree.get(main).allocated >= 1000);
    let zero = find(tree, &format!(r"{root_str}\zero.bin")).expect("zero.bin");
    assert_eq!(tree.get(zero).logical, 0);
    assert!(find(tree, &format!("{root_str}\\caf\u{e9}_\u{65e5}\u{672c}.txt")).is_some());
    let empty = find(tree, &format!(r"{root_str}\empty")).expect("empty");
    assert_eq!(tree.get(empty).child_count(), 0);

    let deep_path = format!(r"{root_str}{}\deep.dat", r"\long_directory_name".repeat(15));
    let deep = find(tree, &deep_path).expect("deep.dat");
    assert_eq!(tree.get(deep).logical, 5000);
}
