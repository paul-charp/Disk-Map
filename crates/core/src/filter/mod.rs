//! Filter strings: parsing and evaluation over a [`Tree`](crate::tree::Tree).

mod eval;
mod parse;
#[cfg(test)]
mod tests;

use std::fmt;

pub use eval::{FilterCtx, Filtered, wildcard_match};

/// Error for one condition of a filter string.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FilterError {
    /// Text of the offending condition.
    pub condition: String,
    /// Short reason.
    pub reason: String,
}

impl fmt::Display for FilterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.condition, self.reason)
    }
}

impl std::error::Error for FilterError {}

#[derive(Clone, Copy, Debug)]
struct TagSpec {
    include: u8,
    exclude: u8,
}

impl TagSpec {
    fn matches(self, mask: u8) -> bool {
        (self.include == 0 || mask & self.include != 0) && mask & self.exclude == 0
    }
}

#[derive(Clone, Debug)]
enum Selector {
    Mask(String),
    Tag(TagSpec),
    Class(usize),
}

#[derive(Clone, Copy, Debug)]
struct AttrCond {
    include: u32,
    exclude: u32,
    negated: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Cmp {
    Greater,
    Less,
}

#[derive(Clone, Copy, Debug)]
struct SizeCond {
    logical: bool,
    cmp: Cmp,
    bytes: u64,
    negated: bool,
}

#[derive(Clone, Copy, Debug)]
enum TimeKind {
    Creation,
    Modify,
    Access,
}

#[derive(Clone, Copy, Debug)]
struct AgeCond {
    time: TimeKind,
    cmp: Cmp,
    ticks: i64,
    negated: bool,
}

/// A parsed filter.
#[derive(Clone, Debug, Default)]
pub struct Filter {
    text: String,
    selectors: Vec<Selector>,
    excludes: Vec<Selector>,
    sizes: Vec<SizeCond>,
    ages: Vec<AgeCond>,
    attrs: Vec<AttrCond>,
    folders: Vec<String>,
    positive_folders: u64,
    negative_folders: u64,
    uses_tags: bool,
}

impl Filter {
    /// The filter that matches everything.
    pub fn void() -> Self {
        Self::default()
    }

    /// Original filter text.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// True if the filter has no condition.
    pub fn is_void(&self) -> bool {
        self.selectors.is_empty()
            && self.excludes.is_empty()
            && self.sizes.is_empty()
            && self.ages.is_empty()
            && self.attrs.is_empty()
            && self.folders.is_empty()
    }

    /// True if the result depends on tags.
    pub fn uses_tags(&self) -> bool {
        self.uses_tags
    }
}
