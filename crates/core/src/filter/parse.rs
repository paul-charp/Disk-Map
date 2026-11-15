//! Filter string parser.

use super::{
    AgeCond, AttrCond, Cmp, DeltaCond, Filter, FilterError, Selector, SizeCond, TagSpec, TimeKind,
    status_bit,
};
use crate::classes::FileClasses;
use crate::diff::Status;

const FILE_ATTRIBUTE_READONLY: u32 = 0x1;
const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;
const FILE_ATTRIBUTE_SYSTEM: u32 = 0x4;
const FILE_ATTRIBUTE_ARCHIVE: u32 = 0x20;
const FILE_ATTRIBUTE_TEMPORARY: u32 = 0x100;
const FILE_ATTRIBUTE_SPARSE_FILE: u32 = 0x200;
const FILE_ATTRIBUTE_OFFLINE: u32 = 0x1000;
const FILE_ATTRIBUTE_NOT_CONTENT_INDEXED: u32 = 0x2000;
const FILE_ATTRIBUTE_ENCRYPTED: u32 = 0x4000;
const FILE_ATTRIBUTE_COMPRESSED: u32 = 0x800;

const ATTRS: &[(&[&str], u32)] = &[
    (
        &["archive", "archi", "arch", "arc", "ar", "a"],
        FILE_ATTRIBUTE_ARCHIVE,
    ),
    (&["system", "sys", "s"], FILE_ATTRIBUTE_SYSTEM),
    (
        &["readonly", "rdonly", "ronly", "rdo", "ro", "r"],
        FILE_ATTRIBUTE_READONLY,
    ),
    (&["hidden", "hidn", "hid", "h"], FILE_ATTRIBUTE_HIDDEN),
    (
        &["compressed", "comprsd", "compr", "cpr", "c"],
        FILE_ATTRIBUTE_COMPRESSED,
    ),
    (
        &[
            "encrypted",
            "encrptd",
            "encptd",
            "enc",
            "e",
            "crypted",
            "crypt",
            "cptd",
        ],
        FILE_ATTRIBUTE_ENCRYPTED,
    ),
    (&["offline", "off", "ol", "o"], FILE_ATTRIBUTE_OFFLINE),
    (&["temporary", "temp", "tmp", "t"], FILE_ATTRIBUTE_TEMPORARY),
    (
        &["notindexed", "notindx", "notidx", "noidx", "nidx", "ni"],
        FILE_ATTRIBUTE_NOT_CONTENT_INDEXED,
    ),
    (&["sparse", "sp"], FILE_ATTRIBUTE_SPARSE_FILE),
];

const LEGACY_TAGS: &[&str] = &[
    "1", "2", "3", "4", "r", "y", "g", "b", "a", "red", "yellow", "green", "blue", "all",
];

const TICKS_PER_SEC: f64 = 10_000_000.0;
const MAX_FOLDER_MASKS: usize = 64;

const SIZE_KEYWORDS: &[(&str, bool)] = &[
    ("disksize", false),
    ("clustersize", false),
    ("filesize", true),
    ("logicalsize", true),
    ("size", true),
];

const AGE_KEYWORDS: &[(&str, TimeKind)] = &[
    ("creation", TimeKind::Creation),
    ("modify", TimeKind::Modify),
    ("modification", TimeKind::Modify),
    ("access", TimeKind::Access),
];

impl Filter {
    /// Parses a filter string; conditions are separated by `;`.
    pub fn parse(text: &str, classes: &FileClasses) -> Result<Filter, FilterError> {
        let mut filter = Filter {
            text: text.to_string(),
            ..Filter::default()
        };
        for raw in text.split(';') {
            let cond = raw.trim();
            if cond.is_empty() {
                continue;
            }
            filter
                .add_condition(cond, classes)
                .map_err(|reason| FilterError {
                    condition: cond.to_string(),
                    reason,
                })?;
        }
        Ok(filter)
    }

    fn add_condition(&mut self, cond: &str, classes: &FileClasses) -> Result<(), String> {
        let (negated, body) = match cond.strip_prefix('|') {
            Some(rest) => (true, rest.trim_start()),
            None => (false, cond),
        };
        if body.is_empty() {
            return Err("empty condition".into());
        }
        let lower = body.to_lowercase();

        let tag_spec = [":tags:", ":tag:"]
            .iter()
            .find_map(|p| lower.strip_prefix(p))
            .or_else(|| lower.strip_prefix(':').filter(|s| LEGACY_TAGS.contains(s)));
        if let Some(spec) = tag_spec {
            let tag = parse_tag_spec(spec)?;
            self.uses_tags = true;
            self.push_selector(negated, Selector::Tag(tag));
            return Ok(());
        }

        if let Some(spec) = [":attrs:", ":attr:"]
            .iter()
            .find_map(|p| lower.strip_prefix(p))
        {
            self.attrs.push(parse_attr_spec(spec, negated)?);
            return Ok(());
        }

        if let Some(word) = lower.strip_prefix("is:") {
            let bit = status_bit(parse_status(word.trim())?);
            if negated {
                self.status_exclude |= bit;
            } else {
                self.status_include |= bit;
            }
            return Ok(());
        }

        if let Some(spec) = lower.strip_prefix("delta:") {
            let (cmp, bytes) = parse_delta(spec.trim())?;
            self.deltas.push(DeltaCond {
                cmp,
                bytes,
                negated,
            });
            return Ok(());
        }

        if lower.starts_with(":class:") {
            let name = body.get(7..).unwrap_or("").trim();
            let index = classes
                .by_name(name)
                .ok_or_else(|| format!("unknown class '{name}'"))?;
            self.push_selector(negated, Selector::Class(index));
            return Ok(());
        }

        if !body.starts_with('\\')
            && let Some(pos) = body.find(['<', '>'])
        {
            return self.add_comparison(body, pos, negated);
        }

        if let Some(mask) = body.strip_prefix('\\') {
            if mask.is_empty() {
                return Err("empty folder mask".into());
            }
            if self.folders.len() >= MAX_FOLDER_MASKS {
                return Err("too many folder masks".into());
            }
            let bit = 1u64 << self.folders.len();
            if negated {
                self.negative_folders |= bit;
            } else {
                self.positive_folders |= bit;
            }
            self.folders.push(mask.to_string());
            return Ok(());
        }

        self.push_selector(negated, Selector::Mask(body.to_string()));
        Ok(())
    }

    fn push_selector(&mut self, negated: bool, selector: Selector) {
        if negated {
            self.excludes.push(selector);
        } else {
            self.selectors.push(selector);
        }
    }

    fn add_comparison(&mut self, body: &str, pos: usize, negated: bool) -> Result<(), String> {
        let keyword = body[..pos].trim().to_lowercase();
        let cmp = if body.as_bytes()[pos] == b'>' {
            Cmp::Greater
        } else {
            Cmp::Less
        };
        if !keyword.chars().all(|c| c.is_ascii_alphabetic()) {
            return Err(format!("unknown keyword '{keyword}'"));
        }
        let rest = body[pos + 1..].trim();
        let num_end = rest
            .find(|c: char| !(c.is_ascii_digit() || c == '.'))
            .unwrap_or(rest.len());
        let number: f64 = rest[..num_end]
            .parse()
            .map_err(|_| "expected a number".to_string())?;
        let unit = rest[num_end..].trim().to_lowercase();
        if unit.is_empty() {
            return Err("missing unit".into());
        }

        if let Some(mult) = size_unit(&unit) {
            let logical = if keyword.is_empty() {
                false
            } else {
                SIZE_KEYWORDS
                    .iter()
                    .find(|(cand, _)| fuzzy(&keyword, cand))
                    .map(|&(_, logical)| logical)
                    .ok_or_else(|| format!("unknown size keyword '{keyword}'"))?
            };
            self.sizes.push(SizeCond {
                logical,
                cmp,
                bytes: (number * mult).round() as u64,
                negated,
            });
        } else if let Some(secs) = age_unit(&unit) {
            let time = if keyword.is_empty() {
                TimeKind::Modify
            } else {
                AGE_KEYWORDS
                    .iter()
                    .find(|(cand, _)| fuzzy(&keyword, cand))
                    .map(|&(_, time)| time)
                    .ok_or_else(|| format!("unknown age keyword '{keyword}'"))?
            };
            self.ages.push(AgeCond {
                time,
                cmp,
                ticks: (number * secs * TICKS_PER_SEC).round() as i64,
                negated,
            });
        } else {
            return Err(format!("unknown unit '{unit}'"));
        }
        Ok(())
    }
}

fn parse_status(word: &str) -> Result<Status, String> {
    Ok(match word {
        "new" => Status::New,
        "deleted" => Status::Deleted,
        "grown" => Status::Grown,
        "shrunk" => Status::Shrunk,
        "same" => Status::Same,
        _ => return Err(format!("unknown status '{word}'")),
    })
}

/// `>1GB` or `<-100MB`: a comparison and a signed size.
fn parse_delta(spec: &str) -> Result<(Cmp, i64), String> {
    let cmp = match spec.chars().next() {
        Some('>') => Cmp::Greater,
        Some('<') => Cmp::Less,
        _ => return Err("expected > or <".into()),
    };
    let rest = spec[1..].trim();
    let (sign, rest) = match rest.strip_prefix('-') {
        Some(r) => (-1.0, r.trim_start()),
        None => (1.0, rest.strip_prefix('+').unwrap_or(rest).trim_start()),
    };
    let num_end = rest
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(rest.len());
    let number: f64 = rest[..num_end]
        .parse()
        .map_err(|_| "expected a number".to_string())?;
    let mult = size_unit(rest[num_end..].trim()).ok_or("expected a size unit")?;
    Ok((cmp, (sign * number * mult).round() as i64))
}

fn size_unit(unit: &str) -> Option<f64> {
    let shift = match unit {
        "b" => 0,
        "kb" => 10,
        "mb" => 20,
        "gb" => 30,
        "tb" => 40,
        _ => return None,
    };
    Some((1u64 << shift) as f64)
}

fn age_unit(unit: &str) -> Option<f64> {
    Some(match unit {
        "seconds" | "secs" | "sec" | "s" => 1.0,
        "minutes" | "mins" | "min" | "m" => 60.0,
        "hours" | "hour" | "h" => 3600.0,
        "days" | "day" | "d" => 86_400.0,
        "weeks" | "week" | "w" => 7.0 * 86_400.0,
        "months" | "month" => 30.0 * 86_400.0,
        "years" | "year" | "y" => 365.0 * 86_400.0,
        _ => return None,
    })
}

/// Typed keyword starts with the candidate's first letter and its letters appear in order.
fn fuzzy(typed: &str, candidate: &str) -> bool {
    let (Some(t0), Some(c0)) = (typed.chars().next(), candidate.chars().next()) else {
        return false;
    };
    if t0 != c0 {
        return false;
    }
    let mut chars = candidate.chars();
    typed.chars().all(|t| chars.any(|c| c == t))
}

/// Splits `+a-b,c` into `(included, word)` pairs.
fn parse_list<T>(
    spec: &str,
    lookup: impl Fn(&str) -> Option<T>,
    what: &str,
) -> Result<Vec<(bool, T)>, String> {
    let mut out = Vec::new();
    let mut include = true;
    let mut start = 0;
    let mut flush = |word: &str, include: bool| -> Result<(), String> {
        if word.is_empty() {
            return Ok(());
        }
        let value = lookup(word).ok_or_else(|| format!("unknown {what} '{word}'"))?;
        out.push((include, value));
        Ok(())
    };
    for (i, c) in spec.char_indices() {
        if matches!(c, '+' | '-' | ',') {
            flush(&spec[start..i], include)?;
            include = c != '-';
            start = i + 1;
        }
    }
    flush(&spec[start..], include)?;
    if out.is_empty() {
        return Err(format!("empty {what} list"));
    }
    Ok(out)
}

fn tag_mask(word: &str) -> Option<u8> {
    Some(match word {
        "1" | "r" | "red" => 1,
        "2" | "y" | "yellow" => 2,
        "3" | "g" | "green" => 4,
        "4" | "b" | "blu" | "blue" => 8,
        "a" | "all" => 0xF,
        _ => return None,
    })
}

fn parse_tag_spec(spec: &str) -> Result<TagSpec, String> {
    let mut tag = TagSpec {
        include: 0,
        exclude: 0,
    };
    for (include, mask) in parse_list(spec, tag_mask, "colour")? {
        if include {
            tag.include |= mask;
        } else {
            tag.exclude |= mask;
        }
    }
    Ok(tag)
}

fn attr_bit(word: &str) -> Option<u32> {
    ATTRS
        .iter()
        .find(|(names, _)| names.contains(&word))
        .map(|&(_, bit)| bit)
}

fn parse_attr_spec(spec: &str, negated: bool) -> Result<AttrCond, String> {
    let mut cond = AttrCond {
        include: 0,
        exclude: 0,
        negated,
    };
    for (include, bit) in parse_list(spec, attr_bit, "attribute")? {
        if include {
            cond.include |= bit;
        } else {
            cond.exclude |= bit;
        }
    }
    Ok(cond)
}
