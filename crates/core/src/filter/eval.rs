//! Filter evaluation over a tree.

use super::{Cmp, Filter, Selector, TimeKind};
use crate::classes::FileClasses;
use crate::tags::Tags;
use crate::tree::{NodeId, NodeKind, Tree};

/// Inputs of an evaluation besides the tree.
pub struct FilterCtx<'a> {
    pub tags: &'a Tags,
    pub classes: &'a FileClasses,
    /// Current time as a Windows FILETIME.
    pub now: i64,
}

/// Result of applying a filter.
#[derive(Clone, Debug, Default)]
pub struct Filtered {
    /// Allocated bytes of matching files, aggregated for directories.
    pub sizes: Vec<u64>,
    /// Logical bytes of matching files, aggregated for directories.
    pub logical: Vec<u64>,
    /// Number of matching files.
    pub matched_files: u64,
    /// Directories whose own name matches a positive folder mask.
    pub bold_dirs: Vec<NodeId>,
}

fn char_at(s: &str, i: usize) -> Option<(char, usize)> {
    s[i..].chars().next().map(|c| (c, c.len_utf8()))
}

fn eq_ignore_case(a: char, b: char) -> bool {
    a == b || a.to_lowercase().eq(b.to_lowercase())
}

/// Case-insensitive wildcard match: `?` is one character, `*` zero or more.
pub fn wildcard_match(pattern: &str, name: &str) -> bool {
    let (mut pi, mut ni) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    loop {
        let pc = char_at(pattern, pi);
        if let Some(('*', len)) = pc {
            pi += len;
            star = Some((pi, ni));
            continue;
        }
        let Some((nc, nlen)) = char_at(name, ni) else {
            return pc.is_none();
        };
        if let Some((c, plen)) = pc
            && (c == '?' || eq_ignore_case(c, nc))
        {
            pi += plen;
            ni += nlen;
            continue;
        }
        let Some((star_pi, star_ni)) = star else {
            return false;
        };
        let skip = char_at(name, star_ni).map_or(1, |(_, l)| l);
        star = Some((star_pi, star_ni + skip));
        pi = star_pi;
        ni = star_ni + skip;
    }
}

fn folder_name(tree: &Tree, id: NodeId) -> &str {
    let name = tree.name(id);
    if tree.parent(id).is_some() {
        return name;
    }
    let trimmed = name.trim_end_matches('\\');
    trimmed.rsplit('\\').next().unwrap_or(trimmed)
}

fn compare(cmp: Cmp, value: i128, limit: i128) -> bool {
    match cmp {
        Cmp::Greater => value > limit,
        Cmp::Less => value < limit,
    }
}

impl Filter {
    /// Applies the filter: one forward pass for inherited state and matching, one reverse pass
    /// to aggregate sizes into directories.
    pub fn apply(&self, tree: &Tree, ctx: &FilterCtx) -> Filtered {
        let n = tree.len();
        if self.is_void() {
            let mut sizes = Vec::with_capacity(n);
            let mut logical = Vec::with_capacity(n);
            for i in 0..n {
                let node = tree.get(NodeId(i as u32));
                sizes.push(node.allocated);
                logical.push(node.logical);
            }
            return Filtered {
                sizes,
                logical,
                matched_files: tree.file_count(),
                bold_dirs: Vec::new(),
            };
        }

        let mut out = Filtered {
            sizes: vec![0; n],
            logical: vec![0; n],
            ..Filtered::default()
        };
        let mut inherited_tags = if self.uses_tags {
            vec![0u8; n]
        } else {
            Vec::new()
        };
        let mut inherited_folders = if self.folders.is_empty() {
            Vec::new()
        } else {
            vec![0u64; n]
        };

        for i in 0..n {
            let id = NodeId(i as u32);
            let node = tree.get(id);
            let parent = tree.parent(id).map(|p| p.0 as usize);

            let mut tag_mask = 0;
            if self.uses_tags {
                tag_mask = parent.map_or(0, |p| inherited_tags[p]) | ctx.tags.mask(id);
                inherited_tags[i] = tag_mask;
            }

            let mut folder_bits = 0;
            if !self.folders.is_empty() {
                folder_bits = parent.map_or(0, |p| inherited_folders[p]);
                if node.kind != NodeKind::File {
                    let own = self.own_folder_bits(folder_name(tree, id));
                    if own & self.positive_folders != 0 {
                        out.bold_dirs.push(id);
                    }
                    inherited_folders[i] = folder_bits | own;
                }
            }

            if node.kind == NodeKind::File
                && self.file_matches(tree, id, tag_mask, folder_bits, ctx)
            {
                out.sizes[i] = node.allocated;
                out.logical[i] = node.logical;
                out.matched_files += 1;
            }
        }

        for i in (1..n).rev() {
            let (size, logical) = (out.sizes[i], out.logical[i]);
            if size != 0 || logical != 0 {
                let p = tree.parent(NodeId(i as u32)).map_or(0, |p| p.0 as usize);
                out.sizes[p] += size;
                out.logical[p] += logical;
            }
        }
        out
    }

    fn own_folder_bits(&self, name: &str) -> u64 {
        let mut bits = 0;
        for (i, mask) in self.folders.iter().enumerate() {
            if wildcard_match(mask, name) {
                bits |= 1 << i;
            }
        }
        bits
    }

    fn selector_matches(
        sel: &Selector,
        tree: &Tree,
        id: NodeId,
        tag_mask: u8,
        ctx: &FilterCtx,
    ) -> bool {
        match sel {
            Selector::Mask(mask) => wildcard_match(mask, tree.name(id)),
            Selector::Tag(spec) => spec.matches(tag_mask),
            Selector::Class(c) => ctx.classes.class_of(tree.name(id)) == Some(*c),
        }
    }

    fn file_matches(
        &self,
        tree: &Tree,
        id: NodeId,
        tag_mask: u8,
        folder_bits: u64,
        ctx: &FilterCtx,
    ) -> bool {
        let node = tree.get(id);
        if !self.selectors.is_empty()
            && !self
                .selectors
                .iter()
                .any(|s| Self::selector_matches(s, tree, id, tag_mask, ctx))
        {
            return false;
        }
        if self
            .excludes
            .iter()
            .any(|s| Self::selector_matches(s, tree, id, tag_mask, ctx))
        {
            return false;
        }
        for c in &self.sizes {
            let value = if c.logical {
                node.logical
            } else {
                node.allocated
            };
            if compare(c.cmp, value.into(), c.bytes.into()) == c.negated {
                return false;
            }
        }
        if !self.ages.is_empty() {
            let times = tree.times(id);
            for c in &self.ages {
                let t = match c.time {
                    TimeKind::Creation => times.ctime,
                    TimeKind::Modify => times.mtime,
                    TimeKind::Access => times.atime,
                };
                let age = i128::from(ctx.now) - i128::from(t);
                if compare(c.cmp, age, c.ticks.into()) == c.negated {
                    return false;
                }
            }
        }
        for c in &self.attrs {
            let ok = node.attrs & c.include == c.include && node.attrs & c.exclude == 0;
            if ok == c.negated {
                return false;
            }
        }
        folder_bits & self.positive_folders == self.positive_folders
            && folder_bits & self.negative_folders == 0
    }
}
