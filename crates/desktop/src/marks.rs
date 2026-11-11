//! Recent changes of a tab, flashed on the map.

use diskmap_core::tree::{NodeId, Tree};
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// How long a change is remembered; the front end fades it out sooner.
const WINDOW: Duration = Duration::from_millis(2000);
const DIRECT: i32 = 255;
const INHERITED: i32 = 110;
const WARN_BIT: i32 = 1 << 15;
const AGE_MASK: i32 = WARN_BIT - 1;
const WEIGHT_SHIFT: u32 = 16;

/// Changed nodes with the time and colour of their last change.
#[derive(Default)]
pub struct Marks {
    nodes: HashMap<NodeId, (Instant, bool)>,
}

/// Packs a highlight: weight `0..=255` in bits 16.., the warning colour in bit 15 and the age in milliseconds below.
fn pack(weight: i32, warn: bool, age_ms: i32) -> i32 {
    weight << WEIGHT_SHIFT | if warn { WARN_BIT } else { 0 } | age_ms.min(AGE_MASK)
}

/// Keeps the stronger of two packed highlights, the newer one on equal weight.
fn merge(slot: &mut i32, new: i32) {
    let (old_w, new_w) = (*slot >> WEIGHT_SHIFT, new >> WEIGHT_SHIFT);
    if new_w > old_w || (new_w == old_w && new & AGE_MASK < *slot & AGE_MASK) {
        *slot = new;
    }
}

impl Marks {
    /// Remembers changed nodes; `warn` marks shrunk and removed ones.
    pub fn add(&mut self, changed: impl IntoIterator<Item = (NodeId, bool)>, now: Instant) {
        self.nodes
            .retain(|_, (at, _)| now.saturating_duration_since(*at) < WINDOW);
        for (node, warn) in changed {
            self.nodes.insert(node, (now, warn));
        }
    }

    /// Packed highlight per item of a layout, or nothing when no change is recent.
    ///
    /// A changed node counts fully; its ancestors that are painted count less, so activity inside
    /// a closed folder is visible on the folder.
    pub fn paint(&self, tree: &Tree, index_of: &[u32], items: usize, now: Instant) -> Vec<i32> {
        let mut out = Vec::new();
        for (&node, &(at, warn)) in &self.nodes {
            let age = now.saturating_duration_since(at);
            if age >= WINDOW {
                continue;
            }
            out.resize(items, 0);
            let age = age.as_millis() as i32;
            let mut weight = DIRECT;
            let mut cur = Some(node);
            while let Some(n) = cur {
                if let Some(&i) = index_of.get(n.0 as usize).filter(|&&i| i != u32::MAX) {
                    merge(&mut out[i as usize], pack(weight, warn, age));
                }
                weight = INHERITED;
                cur = tree.parent(n);
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use diskmap_core::tree::{NewEntry, NodeKind, Times};

    fn entry(name: &str, kind: NodeKind) -> NewEntry<'_> {
        NewEntry {
            name,
            kind,
            attrs: 0,
            logical: 1,
            allocated: 1,
            times: Times::default(),
        }
    }

    #[test]
    fn ancestors_get_a_weaker_highlight() {
        let mut tree = Tree::new("scan");
        let dir = tree.add_child(tree.root(), entry("d", NodeKind::Dir));
        let file = tree.add_child(dir, entry("f", NodeKind::File));
        let index_of = vec![0, 1, u32::MAX];
        let mut marks = Marks::default();
        let now = Instant::now();
        marks.add([(file, true)], now);
        let hl = marks.paint(&tree, &index_of, 2, now);
        assert_eq!(hl[1] >> WEIGHT_SHIFT, INHERITED);
        assert_ne!(hl[1] & WARN_BIT, 0);
        assert_eq!(hl[0] >> WEIGHT_SHIFT, INHERITED);
    }

    #[test]
    fn old_marks_are_dropped() {
        let mut tree = Tree::new("scan");
        let file = tree.add_child(tree.root(), entry("f", NodeKind::File));
        let mut marks = Marks::default();
        let then = Instant::now();
        marks.add([(file, false)], then);
        assert!(marks.paint(&tree, &[0, 1], 2, then + WINDOW).is_empty());
    }
}
