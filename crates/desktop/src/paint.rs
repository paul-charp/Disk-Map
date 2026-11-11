//! Binary paint list sent to the front end.
//!
//! Layout of the buffer, all little-endian `i32`:
//! - header: item count, label blob bytes, view root node, layout micros;
//! - items: x, y, w, h, node, meta, parent index, highlight
//!   (meta = depth | class << 8 | flags << 16; highlight = weight << 16 | warning << 15 | age in ms, 0 for none);
//! - label blob: UTF-8 labels joined by `\n`, one per item flagged [`F_LABEL`], in item order.
//!   A label is `name\u{1}size`. Item 0 is the view root; its rect lies outside the area.

use crate::fmt;
use crate::session::Session;
use diskmap_core::classes::FileClasses;
use diskmap_core::layout::{Item, ItemKind, LayoutParams, Rect, layout};
use diskmap_core::tree::{NodeId, NodeKind};
use std::time::Instant;

/// Node id of the free-space item.
pub const FREE_ID: i32 = -1;
/// Node id of the unknown-space item.
pub const UNKNOWN_ID: i32 = -2;

const F_FILE: u32 = 1;
const F_DIR: u32 = 2;
const F_DRILLED: u32 = 4;
const F_FREE: u32 = 8;
const F_UNKNOWN: u32 = 16;
const F_SELECTED: u32 = 32;
const F_BOLD: u32 = 64;
const F_LABEL: u32 = 128;
const TAG_SHIFT: u32 = 8;
const NO_CLASS: u32 = 255;
const HEADER_INTS: usize = 4;
const ITEM_INTS: usize = 8;

/// Pixel metrics of the map, in device pixels; the front end derives its own from the same `dpr`.
struct Metrics {
    gap: f32,
    padding: f32,
    header: f32,
    min_px: f32,
    label_w: f32,
    label_h: f32,
}

fn metrics(cx: &PaintCtx<'_>) -> Metrics {
    let (dpr, text) = (cx.dpr, cx.text_scale);
    let gap = (2.0 * dpr).round().max(2.0);
    let padding = ((3.0 * dpr).round() - gap / 2.0).max(1.0);
    Metrics {
        gap,
        padding,
        header: (18.0 * dpr).round() - padding,
        min_px: (cx.min_tile as f32 * dpr).round().max(2.0),
        label_w: (32.0 * dpr * text).round(),
        label_h: (24.0 * dpr * text).round(),
    }
}

/// Everything the encoder reads besides the session.
pub struct PaintCtx<'a> {
    pub classes: &'a FileClasses,
    /// Minimum tile size in CSS pixels.
    pub min_tile: u32,
    /// Scale of the label sizes.
    pub text_scale: f32,
    pub width: u32,
    pub height: u32,
    pub dpr: f32,
    /// Include the highlight of recent changes.
    pub highlight: bool,
}

struct Encoder<'a> {
    s: &'a Session,
    cx: &'a PaintCtx<'a>,
    free: u64,
    unknown: u64,
    index_of: Vec<u32>,
    highlight: Vec<i32>,
    ints: Vec<i32>,
    blob: Vec<u8>,
    labels: usize,
}

fn snap(r: Rect) -> [i32; 4] {
    let x0 = r.x.round() as i32;
    let y0 = r.y.round() as i32;
    let x1 = (r.x + r.w).round() as i32;
    let y1 = (r.y + r.h).round() as i32;
    [x0, y0, (x1 - x0).max(1), (y1 - y0).max(1)]
}

impl Encoder<'_> {
    fn name(&self, id: NodeId) -> String {
        self.s.scan.tree().name(id).replace('\n', " ")
    }

    fn label(&self, kind: ItemKind) -> String {
        let (name, size) = match kind {
            ItemKind::Node(id) => (self.name(id), self.s.size_of(id)),
            ItemKind::Free => ("Free".to_string(), self.free),
            ItemKind::Unknown => ("Unknown".to_string(), self.unknown),
        };
        format!("{name}\u{1}{}", fmt::size(size))
    }

    fn describe(&self, kind: ItemKind, item: &Item) -> (i32, u32, u32) {
        let tree = self.s.scan.tree();
        let mut flags = if item.expanded { F_DRILLED } else { 0 };
        let id = match kind {
            ItemKind::Free => return (FREE_ID, flags | F_FREE, NO_CLASS),
            ItemKind::Unknown => return (UNKNOWN_ID, flags | F_UNKNOWN, NO_CLASS),
            ItemKind::Node(id) => id,
        };
        let mut class = NO_CLASS;
        if tree.get(id).kind == NodeKind::File {
            flags |= F_FILE;
            if self.s.view.class_colors {
                let other = self.cx.classes.classes().len();
                class = self.cx.classes.class_of(tree.name(id)).unwrap_or(other) as u32;
            }
        } else {
            flags |= F_DIR;
        }
        if self.s.selection == Some(id) {
            flags |= F_SELECTED;
        }
        if self.s.filter.bold.contains(&id) {
            flags |= F_BOLD;
        }
        flags |= u32::from(self.s.tags.raw(id)) << TAG_SHIFT;
        (id.0 as i32, flags, class)
    }

    fn parent(&self, kind: ItemKind, i: usize) -> i32 {
        if i == 0 {
            return -1;
        }
        let ItemKind::Node(id) = kind else { return 0 };
        let parent = self.s.scan.tree().parent(id);
        parent.map_or(0, |p| self.index_of[p.0 as usize] as i32)
    }

    fn push(&mut self, i: usize, item: &Item, m: &Metrics) {
        let [x, y, w, h] = snap(item.rect);
        let (node, mut flags, class) = self.describe(item.kind, item);
        let wide = item.rect.w >= m.label_w;
        if i > 0 && wide && (item.expanded || item.rect.h >= m.label_h) {
            if self.labels > 0 {
                self.blob.push(b'\n');
            }
            self.labels += 1;
            let label = self.label(item.kind);
            self.blob.extend_from_slice(label.as_bytes());
            flags |= F_LABEL;
        }
        let meta = u32::from(item.depth) | class << 8 | flags << 16;
        let parent = self.parent(item.kind, i);
        let highlight = self.highlight.get(i).copied().unwrap_or(0);
        self.ints
            .extend_from_slice(&[x, y, w, h, node, meta as i32, parent, highlight]);
    }
}

/// Lays out the view root in a `width` x `height` device-pixel area and encodes the result.
pub fn build(s: &Session, cx: &PaintCtx<'_>) -> Vec<u8> {
    let t0 = Instant::now();
    let m = metrics(cx);
    let (free, unknown) = s.free_unknown();
    let params = LayoutParams {
        max_depth: s.view.detail,
        expand_px: 0.0,
        focus: s.selection,
        min_px: m.min_px,
        header: m.header,
        padding: m.padding,
        gap: m.gap,
        free,
        unknown,
    };
    let edge = m.gap / 2.0;
    let bounds = Rect {
        x: edge - m.padding,
        y: edge - m.padding - m.header,
        w: cx.width as f32 - m.gap + 2.0 * m.padding,
        h: cx.height as f32 - m.gap + 2.0 * m.padding + m.header,
    };
    let tree = s.scan.tree();
    let mut items = Vec::new();
    layout(tree, s.sizes(), s.root, bounds, &params, &mut items);

    let mut index_of = vec![u32::MAX; tree.len()];
    for (i, it) in items.iter().enumerate() {
        if let ItemKind::Node(id) = it.kind {
            index_of[id.0 as usize] = i as u32;
        }
    }
    let highlight = if cx.highlight {
        s.marks.paint(tree, &index_of, items.len(), Instant::now())
    } else {
        Vec::new()
    };
    let mut enc = Encoder {
        s,
        cx,
        free,
        unknown,
        index_of,
        highlight,
        ints: Vec::with_capacity(items.len() * ITEM_INTS),
        blob: Vec::new(),
        labels: 0,
    };
    for (i, it) in items.iter().enumerate() {
        enc.push(i, it, &m);
    }
    let micros = t0.elapsed().as_micros() as i32;
    let header = [
        items.len() as i32,
        enc.blob.len() as i32,
        s.root.0 as i32,
        micros,
    ];
    let mut out = Vec::with_capacity((HEADER_INTS + enc.ints.len()) * 4 + enc.blob.len());
    for v in header.iter().chain(&enc.ints) {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out.extend_from_slice(&enc.blob);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{BackendPref, Config};
    use crate::scan::Scan;
    use crate::session::ViewOpts;
    use std::fs;

    fn scanned_session(root: &std::path::Path) -> Session {
        let (mut scan, rx) = Scan::start(root, None, BackendPref::Walk);
        while !scan.apply(rx.recv().expect("scan events")) {}
        Session::new(1, scan, ViewOpts::from_config(&Config::default()))
    }

    fn word(buf: &[u8], i: usize) -> i32 {
        i32::from_le_bytes(buf[i * 4..i * 4 + 4].try_into().expect("4 bytes"))
    }

    #[test]
    fn encodes_items_and_labels() {
        let dir = tempfile::tempdir().expect("tempdir");
        for (name, len) in [
            ("a/big.bin", 40_000),
            ("a/small.bin", 10_000),
            ("b/c.bin", 20_000),
        ] {
            let path = dir.path().join(name);
            fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
            fs::write(path, vec![0u8; len]).expect("write");
        }
        let session = scanned_session(dir.path());
        let classes = FileClasses::default();
        let cx = PaintCtx {
            classes: &classes,
            min_tile: 4,
            text_scale: 1.0,
            width: 800,
            height: 600,
            dpr: 1.5,
            highlight: true,
        };
        let buf = build(&session, &cx);

        let count = word(&buf, 0) as usize;
        let blob_len = word(&buf, 1) as usize;
        assert!(count >= 4, "root, two folders and files, got {count}");
        assert_eq!(buf.len(), (HEADER_INTS + count * ITEM_INTS) * 4 + blob_len);
        assert_eq!(word(&buf, 2), session.root.0 as i32);

        let blob = std::str::from_utf8(&buf[buf.len() - blob_len..]).expect("utf-8 labels");
        let labelled = (0..count)
            .filter(|&i| word(&buf, HEADER_INTS + i * ITEM_INTS + 5) as u32 >> 16 & F_LABEL != 0)
            .count();
        assert_eq!(blob.split('\n').count(), labelled);
        let first_child = HEADER_INTS + ITEM_INTS;
        assert!(word(&buf, first_child) >= 0 && word(&buf, first_child + 1) >= 0);
    }
}
