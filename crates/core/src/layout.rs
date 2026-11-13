//! Squarified treemap layout.

use crate::tree::{NodeId, NodeKind, Tree};
use std::collections::HashSet;

/// Axis-aligned rectangle.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    /// True if the point lies inside (left and top edges inclusive).
    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x < self.x + self.w && y >= self.y && y < self.y + self.h
    }
}

/// Layout settings.
#[derive(Clone, Copy, Debug, Default)]
pub struct LayoutParams<'a> {
    /// Levels of nesting drawn below the layout root.
    pub max_depth: u32,
    /// Folders at least this wide and tall keep expanding past `max_depth`; 0 disables.
    pub expand_px: f32,
    /// Focused node: it and its ancestors are drilled whatever `max_depth` is.
    pub focus: Option<NodeId>,
    /// Items narrower or shorter than this are not emitted.
    pub min_px: f32,
    /// Height reserved at the top of a drawn folder for its label.
    pub header: f32,
    /// Inset inside a folder.
    pub padding: f32,
    /// Space between siblings.
    pub gap: f32,
    /// Size of the free-space pseudo-item placed among the root's children, when above 0.
    pub free: u64,
    /// Size of the unknown-space pseudo-item placed among the root's children, when above 0.
    pub unknown: u64,
    /// Deleted bytes per folder, by node id: a deleted pseudo-item of that size is placed among its children.
    pub deleted: &'a [u64],
}

/// What a layout item stands for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ItemKind {
    Node(NodeId),
    Free,
    Unknown,
    /// Bytes deleted directly in a folder, in compare mode.
    Deleted(NodeId),
}

/// A laid-out rectangle.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Item {
    pub kind: ItemKind,
    pub rect: Rect,
    pub depth: u16,
    /// Children were laid out inside.
    pub expanded: bool,
}

struct Entry {
    size: u64,
    kind: ItemKind,
}

struct Frame {
    node: NodeId,
    content: Rect,
    depth: u16,
    item: usize,
}

/// Rectangle available to the children of a folder drawn at `rect`.
pub fn content_rect(rect: Rect, params: &LayoutParams) -> Rect {
    Rect {
        x: rect.x + params.padding,
        y: rect.y + params.padding + params.header,
        w: rect.w - 2.0 * params.padding,
        h: rect.h - 2.0 * params.padding - params.header,
    }
}

fn fits(r: Rect, params: &LayoutParams) -> bool {
    r.w > 0.0 && r.h > 0.0 && r.w >= params.min_px && r.h >= params.min_px
}

fn sort_key(kind: ItemKind) -> u64 {
    match kind {
        ItemKind::Node(id) => u64::from(id.0),
        ItemKind::Deleted(_) => u64::MAX - 2,
        ItemKind::Free => u64::MAX - 1,
        ItemKind::Unknown => u64::MAX,
    }
}

fn worst_ratio(sum: f64, max: f64, min: f64, side: f64) -> f64 {
    let s2 = sum * sum;
    let l2 = side * side;
    (l2 * max / s2).max(s2 / (l2 * min))
}

/// Squarified tiling of `areas` (sorted descending) over `rect`, calling `place(index, rect)`.
fn squarify(rect: Rect, areas: &[f64], mut place: impl FnMut(usize, Rect)) {
    let (mut x, mut y) = (f64::from(rect.x), f64::from(rect.y));
    let (mut w, mut h) = (f64::from(rect.w), f64::from(rect.h));
    let n = areas.len();
    let mut i = 0;
    while i < n && w > 0.0 && h > 0.0 {
        let side = w.min(h);
        let mut sum = areas[i];
        let mut current = worst_ratio(sum, areas[i], areas[i], side);
        let mut j = i + 1;
        while j < n {
            let next_sum = sum + areas[j];
            let next = worst_ratio(next_sum, areas[i], areas[j], side);
            if next > current {
                break;
            }
            sum = next_sum;
            current = next;
            j += 1;
        }
        let last = j == n;
        if w >= h {
            let tw = if last { w } else { (sum / h).min(w) };
            let mut cy = y;
            for (k, &a) in areas.iter().enumerate().take(j).skip(i) {
                let ih = if k == j - 1 { y + h - cy } else { a / tw };
                place(k, rect_f32(x, cy, tw, ih));
                cy += ih;
            }
            x += tw;
            w -= tw;
        } else {
            let th = if last { h } else { (sum / w).min(h) };
            let mut cx = x;
            for (k, &a) in areas.iter().enumerate().take(j).skip(i) {
                let iw = if k == j - 1 { x + w - cx } else { a / th };
                place(k, rect_f32(cx, y, iw, th));
                cx += iw;
            }
            y += th;
            h -= th;
        }
        i = j;
    }
}

fn rect_f32(x: f64, y: f64, w: f64, h: f64) -> Rect {
    Rect {
        x: x as f32,
        y: y as f32,
        w: w as f32,
        h: h as f32,
    }
}

fn focus_chain(tree: &Tree, focus: Option<NodeId>) -> HashSet<NodeId> {
    let mut chain = HashSet::new();
    let mut cur = focus;
    while let Some(id) = cur {
        chain.insert(id);
        cur = tree.parent(id);
    }
    chain
}

#[allow(clippy::too_many_arguments)]
fn push_frame(
    stack: &mut Vec<Frame>,
    tree: &Tree,
    params: &LayoutParams,
    chain: &HashSet<NodeId>,
    node: NodeId,
    rect: Rect,
    depth: u16,
    item: usize,
) {
    const HARD_DEPTH: u16 = 32;
    let large = params.expand_px > 0.0 && rect.w >= params.expand_px && rect.h >= params.expand_px;
    let forced = chain.contains(&node);
    let too_deep =
        u32::from(depth) >= params.max_depth && !forced && !(large && depth < HARD_DEPTH);
    if tree.get(node).kind == NodeKind::File || too_deep {
        return;
    }
    let content = content_rect(rect, params);
    if fits(content, params) {
        stack.push(Frame {
            node,
            content,
            depth,
            item,
        });
    }
}

/// Source of node sizes for [`layout`].
#[derive(Clone, Copy, Debug)]
pub enum Sizes<'a> {
    /// Allocated bytes straight from the tree.
    Allocated,
    /// One size per node, indexed by node id; nodes past the end count as empty.
    Slice(&'a [u64]),
    /// Allocated bytes plus an extra amount per node, e.g. deleted bytes in compare mode.
    Plus(&'a [u64]),
    /// [`Sizes::Slice`] plus an extra amount per node.
    SlicePlus(&'a [u64], &'a [u64]),
}

impl Sizes<'_> {
    /// Size of a node.
    pub fn get(&self, tree: &Tree, id: NodeId) -> u64 {
        match self {
            Sizes::Allocated => tree.get(id).allocated,
            Sizes::Slice(s) => at(s, id),
            Sizes::Plus(extra) => tree.get(id).allocated + at(extra, id),
            Sizes::SlicePlus(s, extra) => at(s, id) + at(extra, id),
        }
    }
}

fn at(values: &[u64], id: NodeId) -> u64 {
    values.get(id.0 as usize).copied().unwrap_or(0)
}

/// Lays out the subtree under `root` inside `bounds`; `out` is in paint order.
pub fn layout(
    tree: &Tree,
    sizes: Sizes<'_>,
    root: NodeId,
    bounds: Rect,
    params: &LayoutParams,
    out: &mut Vec<Item>,
) {
    out.clear();
    if (sizes.get(tree, root) == 0 && params.free + params.unknown == 0) || !fits(bounds, params) {
        return;
    }
    out.push(Item {
        kind: ItemKind::Node(root),
        rect: bounds,
        depth: 0,
        expanded: false,
    });
    let mut stack = Vec::new();
    let chain = focus_chain(tree, params.focus);
    push_frame(&mut stack, tree, params, &chain, root, bounds, 0, 0);
    let mut entries: Vec<Entry> = Vec::new();
    let mut areas: Vec<f64> = Vec::new();
    let half_gap = params.gap / 2.0;

    while let Some(frame) = stack.pop() {
        entries.clear();
        for child in tree.children(frame.node) {
            let size = sizes.get(tree, child);
            if size > 0 {
                entries.push(Entry {
                    size,
                    kind: ItemKind::Node(child),
                });
            }
        }
        if let Some(&d) = params.deleted.get(frame.node.0 as usize)
            && d > 0
        {
            entries.push(Entry {
                size: d,
                kind: ItemKind::Deleted(frame.node),
            });
        }
        if frame.depth == 0 {
            if params.free > 0 {
                entries.push(Entry {
                    size: params.free,
                    kind: ItemKind::Free,
                });
            }
            if params.unknown > 0 {
                entries.push(Entry {
                    size: params.unknown,
                    kind: ItemKind::Unknown,
                });
            }
        }
        if entries.is_empty() {
            continue;
        }
        entries.sort_unstable_by(|a, b| {
            b.size
                .cmp(&a.size)
                .then_with(|| sort_key(a.kind).cmp(&sort_key(b.kind)))
        });
        let total: f64 = entries.iter().map(|e| e.size as f64).sum();
        let scale = f64::from(frame.content.w) * f64::from(frame.content.h) / total;
        areas.clear();
        areas.extend(entries.iter().map(|e| e.size as f64 * scale));

        let first_child = out.len();
        let child_depth = frame.depth + 1;
        squarify(frame.content, &areas, |k, tile| {
            let rect = Rect {
                x: tile.x + half_gap,
                y: tile.y + half_gap,
                w: tile.w - params.gap,
                h: tile.h - params.gap,
            };
            if !fits(rect, params) {
                return;
            }
            let kind = entries[k].kind;
            let item = out.len();
            out.push(Item {
                kind,
                rect,
                depth: child_depth,
                expanded: false,
            });
            if let ItemKind::Node(id) = kind {
                push_frame(
                    &mut stack,
                    tree,
                    params,
                    &chain,
                    id,
                    rect,
                    child_depth,
                    item,
                );
            }
        });
        if out.len() > first_child {
            out[frame.item].expanded = true;
        }
    }
}

/// Index of the deepest item containing the point.
pub fn hit_test(items: &[Item], x: f32, y: f32) -> Option<usize> {
    let mut best: Option<usize> = None;
    for (i, item) in items.iter().enumerate() {
        if item.rect.contains(x, y) && best.is_none_or(|b| item.depth >= items[b].depth) {
            best = Some(i);
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::{NewEntry, Times};
    use std::collections::HashMap;

    fn add(t: &mut Tree, p: NodeId, name: &str, kind: NodeKind, size: u64) -> NodeId {
        t.add_child(
            p,
            NewEntry {
                name,
                kind,
                attrs: 0,
                logical: size,
                allocated: size,
                times: Times::default(),
            },
        )
    }

    fn allocated(t: &Tree) -> Vec<u64> {
        (0..t.len())
            .map(|i| t.get(NodeId(i as u32)).allocated)
            .collect()
    }

    fn area(r: &Rect) -> f64 {
        f64::from(r.w) * f64::from(r.h)
    }

    fn overlap(a: &Rect, b: &Rect) -> f64 {
        let w = (a.x + a.w).min(b.x + b.w) - a.x.max(b.x);
        let h = (a.y + a.h).min(b.y + b.h) - a.y.max(b.y);
        if w > 0.0 && h > 0.0 {
            f64::from(w) * f64::from(h)
        } else {
            0.0
        }
    }

    fn inside(inner: &Rect, outer: &Rect) -> bool {
        let eps = 1e-3;
        inner.x >= outer.x - eps
            && inner.y >= outer.y - eps
            && inner.x + inner.w <= outer.x + outer.w + eps
            && inner.y + inner.h <= outer.y + outer.h + eps
    }

    fn flat_params() -> LayoutParams<'static> {
        LayoutParams {
            max_depth: 8,
            ..LayoutParams::default()
        }
    }

    const BOUNDS: Rect = Rect {
        x: 10.0,
        y: 20.0,
        w: 400.0,
        h: 300.0,
    };

    fn sample() -> (Tree, NodeId, NodeId) {
        let mut t = Tree::new("C:\\");
        let root = t.root();
        let a = add(&mut t, root, "a", NodeKind::Dir, 0);
        add(&mut t, a, "a1", NodeKind::File, 4000);
        add(&mut t, a, "a2", NodeKind::File, 2500);
        add(&mut t, a, "a3", NodeKind::File, 500);
        add(&mut t, root, "b", NodeKind::File, 2000);
        let c = add(&mut t, root, "c", NodeKind::Dir, 0);
        add(&mut t, c, "c1", NodeKind::File, 700);
        add(&mut t, c, "c2", NodeKind::File, 300);
        add(&mut t, root, "zero", NodeKind::File, 0);
        (t, a, c)
    }

    #[test]
    fn proportional_and_tiling_without_decoration() {
        let (t, _, _) = sample();
        let sizes = allocated(&t);
        let mut out = Vec::new();
        layout(
            &t,
            Sizes::Slice(&sizes),
            t.root(),
            BOUNDS,
            &flat_params(),
            &mut out,
        );

        assert_eq!(out[0].kind, ItemKind::Node(t.root()));
        assert_eq!(out[0].depth, 0);
        assert!(out[0].expanded);
        let total = sizes[0] as f64;
        let mut sum = 0.0;
        let mut count = 0;
        for item in out.iter().filter(|i| i.depth == 1) {
            let ItemKind::Node(id) = item.kind else {
                panic!("pseudo item");
            };
            let expected = area(&BOUNDS) * sizes[id.0 as usize] as f64 / total;
            assert!((area(&item.rect) - expected).abs() <= expected * 1e-3);
            sum += area(&item.rect);
            count += 1;
        }
        assert_eq!(count, 3);
        assert!((sum - area(&BOUNDS)).abs() <= area(&BOUNDS) * 1e-3);
        assert!(out.iter().all(|i| t.name(match i.kind {
            ItemKind::Node(id) => id,
            _ => unreachable!(),
        }) != "zero"));
    }

    #[test]
    fn paint_order_and_nesting() {
        let (t, a, _) = sample();
        let sizes = allocated(&t);
        let mut out = Vec::new();
        layout(
            &t,
            Sizes::Slice(&sizes),
            t.root(),
            BOUNDS,
            &flat_params(),
            &mut out,
        );
        let pos = |id: NodeId| {
            out.iter()
                .position(|i| i.kind == ItemKind::Node(id))
                .unwrap()
        };
        let a1 = t.children(a).map(pos).min().unwrap();
        assert!(pos(a) < a1);
        assert!(out[pos(a)].expanded);
        assert!(inside(&out[a1].rect, &out[pos(a)].rect));
    }

    #[test]
    fn focus_path_is_drilled_past_max_depth() {
        let (t, a, c) = sample();
        let sizes = allocated(&t);
        let a1 = t.children(a).next().unwrap();
        let params = LayoutParams {
            max_depth: 1,
            ..LayoutParams::default()
        };
        let expanded = |p: &LayoutParams, id: NodeId| {
            let mut out = Vec::new();
            layout(&t, Sizes::Slice(&sizes), t.root(), BOUNDS, p, &mut out);
            out.iter()
                .find(|i| i.kind == ItemKind::Node(id))
                .is_some_and(|i| i.expanded)
        };
        assert!(!expanded(&params, a));
        let focused = LayoutParams {
            focus: Some(a1),
            ..params
        };
        assert!(expanded(&focused, a));
        assert!(!expanded(&focused, c));
        let on_dir = LayoutParams {
            focus: Some(c),
            ..params
        };
        assert!(expanded(&on_dir, c));
    }

    #[test]
    fn max_depth_is_respected() {
        let (t, a, _) = sample();
        let sizes = allocated(&t);
        for max_depth in 0..3u32 {
            let params = LayoutParams {
                max_depth,
                ..LayoutParams::default()
            };
            let mut out = Vec::new();
            layout(
                &t,
                Sizes::Slice(&sizes),
                t.root(),
                BOUNDS,
                &params,
                &mut out,
            );
            assert!(out.iter().all(|i| u32::from(i.depth) <= max_depth));
            assert!(
                out.iter()
                    .filter(|i| u32::from(i.depth) == max_depth)
                    .all(|i| !i.expanded)
            );
            let has_a_child = out
                .iter()
                .any(|i| matches!(i.kind, ItemKind::Node(id) if t.parent(id) == Some(a)));
            assert_eq!(has_a_child, max_depth >= 2);
        }
    }

    #[test]
    fn decorations_keep_children_inside_content() {
        let (t, a, _) = sample();
        let sizes = allocated(&t);
        let params = LayoutParams {
            max_depth: 4,
            min_px: 1.0,
            header: 12.0,
            padding: 3.0,
            gap: 2.0,
            ..LayoutParams::default()
        };
        let mut out = Vec::new();
        layout(
            &t,
            Sizes::Slice(&sizes),
            t.root(),
            BOUNDS,
            &params,
            &mut out,
        );
        let ia = out
            .iter()
            .position(|i| i.kind == ItemKind::Node(a))
            .unwrap();
        let content = content_rect(out[ia].rect, &params);
        let kids: Vec<_> = out
            .iter()
            .filter(|i| matches!(i.kind, ItemKind::Node(id) if t.parent(id) == Some(a)))
            .collect();
        assert_eq!(kids.len(), 3);
        for (n, k) in kids.iter().enumerate() {
            assert!(inside(&k.rect, &content));
            assert!(k.rect.y >= out[ia].rect.y + params.header);
            for o in &kids[n + 1..] {
                assert!(overlap(&k.rect, &o.rect) < 1e-3);
            }
        }
    }

    #[test]
    fn small_folders_are_unexpanded_and_small_items_dropped() {
        let (t, _, _) = sample();
        let sizes = allocated(&t);
        let params = LayoutParams {
            max_depth: 4,
            min_px: 20.0,
            header: 18.0,
            padding: 2.0,
            ..LayoutParams::default()
        };
        let mut out = Vec::new();
        layout(
            &t,
            Sizes::Slice(&sizes),
            t.root(),
            Rect {
                x: 0.0,
                y: 0.0,
                w: 60.0,
                h: 60.0,
            },
            &params,
            &mut out,
        );
        assert!(out.iter().all(|i| i.rect.w >= 20.0 && i.rect.h >= 20.0));
        assert!(out.iter().skip(1).all(|i| !i.expanded));
    }

    #[test]
    fn deleted_items_sit_in_their_folder() {
        let (t, a, _) = sample();
        let mut deleted = vec![0u64; t.len()];
        deleted[a.0 as usize] = 3000;
        let mut extra = vec![0u64; t.len()];
        extra[a.0 as usize] = 3000;
        extra[0] = 3000;
        let params = LayoutParams {
            deleted: &deleted,
            ..flat_params()
        };
        let mut out = Vec::new();
        layout(&t, Sizes::Plus(&extra), t.root(), BOUNDS, &params, &mut out);
        let ghosts: Vec<_> = out
            .iter()
            .filter(|i| i.kind == ItemKind::Deleted(a))
            .collect();
        assert_eq!(ghosts.len(), 1);
        assert_eq!(ghosts[0].depth, 2);
        let total = (Sizes::Plus(&extra)).get(&t, t.root()) as f64;
        let expected = area(&BOUNDS) * 3000.0 / total;
        assert!((area(&ghosts[0].rect) - expected).abs() <= expected * 1e-3);
    }

    #[test]
    fn free_and_unknown_only_at_root() {
        let (t, _, _) = sample();
        let sizes = allocated(&t);
        let params = LayoutParams {
            max_depth: 4,
            free: 3000,
            unknown: 1000,
            ..LayoutParams::default()
        };
        let mut out = Vec::new();
        layout(
            &t,
            Sizes::Slice(&sizes),
            t.root(),
            BOUNDS,
            &params,
            &mut out,
        );
        let free: Vec<_> = out.iter().filter(|i| i.kind == ItemKind::Free).collect();
        let unknown: Vec<_> = out.iter().filter(|i| i.kind == ItemKind::Unknown).collect();
        assert_eq!(free.len(), 1);
        assert_eq!(unknown.len(), 1);
        assert_eq!(free[0].depth, 1);
        assert_eq!(unknown[0].depth, 1);
        let total = sizes[0] as f64 + 4000.0;
        let expected = area(&BOUNDS) * 3000.0 / total;
        assert!((area(&free[0].rect) - expected).abs() <= expected * 1e-3);
    }

    #[test]
    fn hit_test_returns_deepest() {
        let (t, a, _) = sample();
        let sizes = allocated(&t);
        let mut out = Vec::new();
        layout(
            &t,
            Sizes::Slice(&sizes),
            t.root(),
            BOUNDS,
            &flat_params(),
            &mut out,
        );
        let ia = out
            .iter()
            .position(|i| i.kind == ItemKind::Node(a))
            .unwrap();
        let leaf = out
            .iter()
            .position(|i| matches!(i.kind, ItemKind::Node(id) if t.parent(id) == Some(a)))
            .unwrap();
        let r = out[leaf].rect;
        let hit = hit_test(&out, r.x + r.w / 2.0, r.y + r.h / 2.0).unwrap();
        assert_eq!(hit, leaf);
        assert!(out[ia].rect.contains(r.x + 0.1, r.y + 0.1));
        assert_eq!(hit_test(&out, -5.0, -5.0), None);
        let root_hit = hit_test(&out, BOUNDS.x, BOUNDS.y).unwrap();
        assert!(out[root_hit].depth >= 1 || root_hit == 0);
    }

    struct Lcg(u64);

    impl Lcg {
        fn next(&mut self, bound: u64) -> u64 {
            self.0 = self
                .0
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (self.0 >> 33) % bound
        }
    }

    #[test]
    fn randomised_trees_are_contained_and_disjoint() {
        for seed in 1..=8u64 {
            let mut rng = Lcg(seed);
            let mut t = Tree::new("C:\\");
            let mut dirs = vec![t.root()];
            for i in 0..400 {
                let parent = dirs[rng.next(dirs.len() as u64) as usize];
                let name = format!("n{i}");
                if rng.next(10) < 3 {
                    dirs.push(add(&mut t, parent, &name, NodeKind::Dir, 0));
                } else {
                    let bound = rng.next(100_000) + 1;
                    let size = 1 + rng.next(bound);
                    add(&mut t, parent, &name, NodeKind::File, size);
                }
            }
            let sizes = allocated(&t);
            let params = LayoutParams {
                max_depth: 6,
                expand_px: 0.0,
                focus: None,
                min_px: 3.0,
                header: 14.0,
                padding: 2.0,
                gap: 1.0,
                free: 5000,
                unknown: 3000,
                deleted: &[],
            };
            let bounds = Rect {
                x: 0.0,
                y: 0.0,
                w: 1000.0,
                h: 700.0,
            };
            let mut out = Vec::new();
            layout(
                &t,
                Sizes::Slice(&sizes),
                t.root(),
                bounds,
                &params,
                &mut out,
            );
            assert!(out.len() > 10);

            let index: HashMap<u32, usize> = out
                .iter()
                .enumerate()
                .filter_map(|(i, it)| match it.kind {
                    ItemKind::Node(id) => Some((id.0, i)),
                    _ => None,
                })
                .collect();
            let mut groups: HashMap<usize, Vec<usize>> = HashMap::new();
            for (i, item) in out.iter().enumerate().skip(1) {
                let parent = match item.kind {
                    ItemKind::Node(id) => index[&t.parent(id).unwrap().0],
                    _ => {
                        assert_eq!(item.depth, 1);
                        0
                    }
                };
                assert!(parent < i, "parent painted first");
                assert!(out[parent].expanded);
                assert_eq!(out[parent].depth + 1, item.depth);
                assert!(inside(&item.rect, &content_rect(out[parent].rect, &params)));
                assert!(item.rect.w >= params.min_px && item.rect.h >= params.min_px);
                groups.entry(parent).or_default().push(i);
            }
            for members in groups.values() {
                for (n, &a) in members.iter().enumerate() {
                    for &b in &members[n + 1..] {
                        assert!(overlap(&out[a].rect, &out[b].rect) < 1e-2);
                    }
                }
            }
            assert!(out.iter().all(|i| u32::from(i.depth) <= params.max_depth));
        }
    }
}
