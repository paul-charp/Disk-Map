use super::*;
use crate::classes::FileClasses;
use crate::tags::{Tag, Tags};
use crate::tree::{NewEntry, NodeId, NodeKind, Times, Tree};
use std::collections::HashMap;

const NOW: i64 = 133_500_000_000_000_000;
const DAY: i64 = 864_000_000_000;
const KB: u64 = 1024;
const MB: u64 = 1024 * KB;
const GB: u64 = 1024 * MB;

struct Fixture {
    tree: Tree,
    tags: Tags,
    classes: FileClasses,
    ids: HashMap<&'static str, NodeId>,
}

fn dir(t: &mut Tree, ids: &mut HashMap<&'static str, NodeId>, p: NodeId, name: &'static str) {
    let id = t.add_child(
        p,
        NewEntry {
            name,
            kind: NodeKind::Dir,
            attrs: 0x10,
            logical: 0,
            allocated: 0,
            times: Times::default(),
        },
    );
    ids.insert(name, id);
}

#[allow(clippy::too_many_arguments)]
fn file(
    t: &mut Tree,
    ids: &mut HashMap<&'static str, NodeId>,
    p: NodeId,
    name: &'static str,
    logical: u64,
    allocated: u64,
    attrs: u32,
    mtime_days: i64,
    atime_days: i64,
) {
    let id = t.add_child(
        p,
        NewEntry {
            name,
            kind: NodeKind::File,
            attrs,
            logical,
            allocated,
            times: Times {
                ctime: NOW - mtime_days * DAY,
                mtime: NOW - mtime_days * DAY,
                atime: NOW - atime_days * DAY,
            },
        },
    );
    ids.insert(name, id);
}

fn fixture() -> Fixture {
    let mut t = Tree::new("C:\\");
    let mut ids = HashMap::new();
    let root = t.root();
    dir(&mut t, &mut ids, root, "Windows");
    let windows = ids["Windows"];
    dir(&mut t, &mut ids, windows, "temp");
    let temp = ids["temp"];
    dir(&mut t, &mut ids, temp, "data");
    let data = ids["data"];
    dir(&mut t, &mut ids, root, "media");
    let media = ids["media"];
    file(&mut t, &mut ids, data, "abc.txt", 100, 4096, 0x20, 1, 5);
    file(
        &mut t,
        &mut ids,
        windows,
        "notes.txt",
        2 * KB,
        4096,
        0x21,
        400,
        5,
    );
    file(
        &mut t,
        &mut ids,
        media,
        "a.jpg",
        2 * MB,
        2 * MB,
        0x20,
        10,
        5,
    );
    file(
        &mut t,
        &mut ids,
        media,
        "b.gif",
        50 * KB,
        52 * KB,
        0x22,
        200,
        5,
    );
    file(
        &mut t,
        &mut ids,
        media,
        "c.mp3",
        5 * MB,
        5 * MB,
        0x20,
        20,
        5,
    );
    file(
        &mut t,
        &mut ids,
        media,
        "d.zip",
        200 * KB,
        200 * KB,
        0x100,
        800,
        5,
    );
    file(
        &mut t,
        &mut ids,
        root,
        "big.iso",
        2 * GB,
        2 * GB,
        0x20,
        1095,
        5,
    );
    file(
        &mut t,
        &mut ids,
        root,
        "old.log",
        10 * KB,
        12 * KB,
        0,
        500,
        600,
    );

    let mut tags = Tags::new();
    tags.toggle(windows, Tag::Green);
    tags.toggle(ids["a.jpg"], Tag::Red);
    tags.toggle(ids["c.mp3"], Tag::Blue);
    tags.toggle(ids["d.zip"], Tag::Green);
    Fixture {
        tree: t,
        tags,
        classes: FileClasses::default(),
        ids,
    }
}

const FILES: [&str; 8] = [
    "abc.txt",
    "notes.txt",
    "a.jpg",
    "b.gif",
    "c.mp3",
    "d.zip",
    "big.iso",
    "old.log",
];

fn run(f: &Fixture, text: &str) -> (Vec<&'static str>, Filtered) {
    let filter = Filter::parse(text, &f.classes).unwrap_or_else(|e| panic!("{text}: {e}"));
    let ctx = FilterCtx {
        tags: &f.tags,
        classes: &f.classes,
        now: NOW,
        compare: None,
    };
    let res = filter.apply(&f.tree, &ctx);
    let matched: Vec<_> = FILES
        .iter()
        .copied()
        .filter(|n| res.sizes[f.ids[n].0 as usize] > 0)
        .collect();
    assert_eq!(res.matched_files as usize, matched.len(), "{text}");
    (matched, res)
}

fn check(text: &str, expected: &[&str]) {
    let f = fixture();
    let (matched, _) = run(&f, text);
    assert_eq!(matched, expected, "filter {text:?}");
}

fn all_but(excluded: &[&str]) -> Vec<&'static str> {
    FILES
        .iter()
        .copied()
        .filter(|n| !excluded.contains(n))
        .collect()
}

#[test]
fn void_filter_matches_everything() {
    let f = fixture();
    for text in ["", "  ; ;; "] {
        let filter = Filter::parse(text, &f.classes).unwrap();
        assert!(filter.is_void());
        assert!(!filter.uses_tags());
        let ctx = FilterCtx {
            tags: &f.tags,
            classes: &f.classes,
            now: NOW,
            compare: None,
        };
        let res = filter.apply(&f.tree, &ctx);
        assert_eq!(res.matched_files, 8);
        assert_eq!(res.sizes[0], f.tree.get(f.tree.root()).allocated);
    }
}

#[test]
fn file_masks() {
    check("*.jpg", &["a.jpg"]);
    check("|*.jpg", &all_but(&["a.jpg"]));
    check("A.JPG", &["a.jpg"]);
    check("?.gif", &["b.gif"]);
    check("a.jp", &[]);
}

#[test]
fn folder_masks() {
    let f = fixture();
    let (matched, res) = run(&f, "\\temp");
    assert_eq!(matched, ["abc.txt"]);
    assert_eq!(res.bold_dirs, [f.ids["temp"]]);
    check("|\\temp", &all_but(&["abc.txt"]));
    check("\\win*;\\data", &["abc.txt"]);
}

#[test]
fn size_conditions() {
    check(">100kb", &["a.jpg", "c.mp3", "d.zip", "big.iso"]);
    check(
        "size<1mb",
        &["abc.txt", "notes.txt", "b.gif", "d.zip", "old.log"],
    );
    check("dsk>1gb", &["big.iso"]);
    check("size<11kb", &["abc.txt", "notes.txt", "old.log"]);
    check("dsk<11kb", &["abc.txt", "notes.txt"]);
    check(">1.5 mb", &["a.jpg", "c.mp3", "big.iso"]);
}

#[test]
fn age_conditions() {
    check(">1year", &["notes.txt", "d.zip", "big.iso", "old.log"]);
    check("a>1year", &["old.log"]);
    check("<3months", &["abc.txt", "a.jpg", "c.mp3"]);
    check("creation>2y", &["d.zip", "big.iso"]);
}

#[test]
fn tag_conditions() {
    let tagged = ["abc.txt", "notes.txt", "a.jpg", "c.mp3", "d.zip"];
    check(":1", &["a.jpg"]);
    check(":red", &["a.jpg"]);
    check(":all", &tagged);
    check("|:3", &all_but(&["abc.txt", "notes.txt", "d.zip"]));
    check("|:a", &all_but(&tagged));
    check(
        ":tag:red+green-b",
        &["abc.txt", "notes.txt", "a.jpg", "d.zip"],
    );
    check("|:tag:1,3", &["b.gif", "c.mp3", "big.iso", "old.log"]);
    check(":tags:b", &["c.mp3"]);
    check(":tag:-b", &all_but(&["c.mp3"]));
    assert!(
        Filter::parse(":a", &FileClasses::default())
            .unwrap()
            .uses_tags()
    );
}

#[test]
fn attribute_conditions() {
    check(":attr:archive+hidden", &["b.gif"]);
    check(":attr:+a-ro,h", &["b.gif"]);
    check(
        ":attr:a-ro",
        &["abc.txt", "a.jpg", "b.gif", "c.mp3", "big.iso"],
    );
    check("|:attr:temp", &all_but(&["d.zip"]));
    check(":attrs:ro", &["notes.txt"]);
}

#[test]
fn class_conditions() {
    check(":class:Audio", &["c.mp3"]);
    check("|:class:Audio", &all_but(&["c.mp3"]));
    check(":class:audio", &["c.mp3"]);
}

#[test]
fn combined() {
    check("*.jpg;*.gif;>100kb;<6months", &["a.jpg"]);
    check("*.jpg;*.gif", &["a.jpg", "b.gif"]);
    check("*.jpg;:class:Audio", &["a.jpg", "c.mp3"]);
    check("*.jpg;|:1", &[]);
}

#[test]
fn directory_sizes_aggregate() {
    let f = fixture();
    let (_, res) = run(&f, "*.txt");
    let windows = f.ids["Windows"].0 as usize;
    assert_eq!(res.sizes[windows], 8192);
    assert_eq!(res.logical[windows], 100 + 2 * KB);
    assert_eq!(res.sizes[f.ids["media"].0 as usize], 0);
    assert_eq!(res.sizes[0], 8192);
}

#[test]
fn errors() {
    let classes = FileClasses::default();
    for (text, condition) in [
        (">10xx", ">10xx"),
        (":class:Nope", ":class:Nope"),
        ("zzz>1mb", "zzz>1mb"),
        ("*.jpg; zzz>1d ;*.gif", "zzz>1d"),
        (">kb", ">kb"),
        (">10", ">10"),
        (":attr:+bogus", ":attr:+bogus"),
        (":tag:purple", ":tag:purple"),
    ] {
        let err = Filter::parse(text, &classes).unwrap_err();
        assert_eq!(err.condition, condition, "{text}");
        assert!(!err.reason.is_empty());
    }
}

#[test]
fn wildcard_matcher() {
    assert!(wildcard_match("*", ""));
    assert!(wildcard_match("a*b*c", "aXXbYYc"));
    assert!(wildcard_match("a*b*c", "abc"));
    assert!(!wildcard_match("a*b*c", "aXXbYY"));
    assert!(wildcard_match("*.JPG", "photo.jpg"));
    assert!(wildcard_match("???", "abc"));
    assert!(!wildcard_match("???", "ab"));
    assert!(wildcard_match("*abc", "ababc"));
    assert!(wildcard_match("été*", "ÉTÉ.txt"));
    assert!(!wildcard_match("abc", "abcd"));
}

/// Old side for compare filters: big.iso was 1 GB, c.mp3 6 MB, a.jpg did not exist.
fn old_side(f: &Fixture) -> Tree {
    let mut old = f.tree.clone();
    let t = Times::default();
    old.set_file(f.ids["big.iso"], GB, GB, t, 0x20);
    old.set_file(f.ids["c.mp3"], 6 * MB, 6 * MB, t, 0x20);
    old.remove(f.ids["a.jpg"]);
    old
}

fn run_compare(text: &str) -> Vec<&'static str> {
    let f = fixture();
    let old = old_side(&f);
    let cmp = crate::compare::Compare::new(&old, &f.tree);
    let filter = Filter::parse(text, &f.classes).unwrap_or_else(|e| panic!("{text}: {e}"));
    let ctx = FilterCtx {
        tags: &f.tags,
        classes: &f.classes,
        now: NOW,
        compare: Some(&cmp),
    };
    let res = filter.apply(&f.tree, &ctx);
    FILES
        .iter()
        .copied()
        .filter(|n| res.sizes[f.ids[n].0 as usize] > 0)
        .collect()
}

#[test]
fn compare_conditions() {
    assert_eq!(run_compare("is:new"), ["a.jpg"]);
    assert_eq!(run_compare("is:grown"), ["big.iso"]);
    assert_eq!(run_compare("is:shrunk"), ["c.mp3"]);
    assert_eq!(run_compare("is:new;is:grown"), ["a.jpg", "big.iso"]);
    assert_eq!(run_compare("|is:same"), ["a.jpg", "c.mp3", "big.iso"]);
    assert_eq!(run_compare("delta:>1mb"), ["a.jpg", "big.iso"]);
    assert_eq!(run_compare("delta:<-512KB"), ["c.mp3"]);
    assert!(run_compare("is:deleted").is_empty());
    let f = fixture();
    let parsed = Filter::parse("is:deleted", &f.classes).unwrap();
    assert!(parsed.shows_deleted() && parsed.uses_compare() && !parsed.is_void());
}

#[test]
fn compare_conditions_match_nothing_outside_compare_mode() {
    check("is:new", &[]);
    check("delta:>0b", &[]);
    let classes = FileClasses::default();
    assert!(Filter::parse("is:bogus", &classes).is_err());
    assert!(Filter::parse("delta:1gb", &classes).is_err());
}
