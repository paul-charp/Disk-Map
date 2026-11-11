//! Colour tags attached to tree nodes.

use crate::tree::{NodeId, Tree};

/// One of the four colour tags.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Tag {
    Red = 1,
    Yellow = 2,
    Green = 3,
    Blue = 4,
}

impl Tag {
    /// Tag for a raw value 1..=4.
    pub fn from_u8(v: u8) -> Option<Tag> {
        match v {
            1 => Some(Tag::Red),
            2 => Some(Tag::Yellow),
            3 => Some(Tag::Green),
            4 => Some(Tag::Blue),
            _ => None,
        }
    }

    /// Single-bit mask of the tag (bit `value - 1`).
    pub fn bit(self) -> u8 {
        1 << (self as u8 - 1)
    }
}

/// One optional tag per node, kept outside the tree.
#[derive(Clone, Debug, Default)]
pub struct Tags {
    values: Vec<u8>,
    count: usize,
}

impl Tags {
    /// Creates an empty tag set.
    pub fn new() -> Self {
        Self::default()
    }

    /// Own tag of a node.
    pub fn get(&self, id: NodeId) -> Option<Tag> {
        Tag::from_u8(self.raw(id))
    }

    /// Raw own tag value, 0 when untagged.
    pub fn raw(&self, id: NodeId) -> u8 {
        self.values.get(id.0 as usize).copied().unwrap_or(0)
    }

    /// Bit mask of the own tag, 0 when untagged.
    pub fn mask(&self, id: NodeId) -> u8 {
        Tag::from_u8(self.raw(id)).map_or(0, Tag::bit)
    }

    /// Sets the tag, or clears it if the node already has that tag.
    pub fn toggle(&mut self, id: NodeId, tag: Tag) {
        let i = id.0 as usize;
        if i >= self.values.len() {
            self.values.resize(i + 1, 0);
        }
        let slot = &mut self.values[i];
        if *slot == tag as u8 {
            *slot = 0;
            self.count -= 1;
        } else {
            if *slot == 0 {
                self.count += 1;
            }
            *slot = tag as u8;
        }
    }

    /// Clears the tag of `root` and of every descendant.
    pub fn clear_subtree(&mut self, tree: &Tree, root: NodeId) {
        if self.count == 0 {
            return;
        }
        let mut stack = vec![root];
        while let Some(id) = stack.pop() {
            if let Some(slot) = self.values.get_mut(id.0 as usize)
                && *slot != 0
            {
                *slot = 0;
                self.count -= 1;
            }
            stack.extend(tree.children(id));
        }
    }

    /// True if any node carries a tag.
    pub fn any(&self) -> bool {
        self.count > 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::{NewEntry, NodeKind, Times};

    fn add(t: &mut Tree, p: NodeId, name: &str, kind: NodeKind) -> NodeId {
        t.add_child(
            p,
            NewEntry {
                name,
                kind,
                attrs: 0,
                logical: 1,
                allocated: 1,
                times: Times::default(),
            },
        )
    }

    #[test]
    fn toggle_sets_replaces_clears() {
        let mut tags = Tags::new();
        let id = NodeId(5);
        assert_eq!(tags.get(id), None);
        assert!(!tags.any());
        tags.toggle(id, Tag::Red);
        assert_eq!(tags.get(id), Some(Tag::Red));
        assert_eq!(tags.mask(id), 1);
        assert!(tags.any());
        tags.toggle(id, Tag::Blue);
        assert_eq!(tags.get(id), Some(Tag::Blue));
        assert_eq!(tags.mask(id), 8);
        tags.toggle(id, Tag::Blue);
        assert_eq!(tags.get(id), None);
        assert!(!tags.any());
    }

    #[test]
    fn clear_subtree_only_touches_subtree() {
        let mut t = Tree::new("C:\\");
        let root = t.root();
        let a = add(&mut t, root, "a", NodeKind::Dir);
        let b = add(&mut t, a, "b", NodeKind::Dir);
        let f = add(&mut t, b, "f", NodeKind::File);
        let other = add(&mut t, root, "other", NodeKind::File);
        let mut tags = Tags::new();
        tags.toggle(a, Tag::Red);
        tags.toggle(f, Tag::Green);
        tags.toggle(other, Tag::Yellow);
        tags.clear_subtree(&t, a);
        assert_eq!(tags.get(a), None);
        assert_eq!(tags.get(f), None);
        assert_eq!(tags.get(other), Some(Tag::Yellow));
        assert!(tags.any());
        tags.clear_subtree(&t, root);
        assert!(!tags.any());
    }
}
