//! CSV and JSON export of tree rows.

use crate::filter::Filtered;
use crate::tags::{Tag, Tags};
use crate::tree::{NodeId, NodeKind, Tree};
use std::io::{self, Write};

const FILETIME_UNIX_SECS: i64 = 11_644_473_600;
const CSV_HEADER: &str = "path,kind,allocated,logical,files,modified,tag";

/// Output format.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Csv,
    Json,
}

impl Format {
    /// Format for a name such as `csv` or `json`.
    pub fn parse(name: &str) -> Option<Format> {
        match name.to_ascii_lowercase().as_str() {
            "csv" => Some(Format::Csv),
            "json" => Some(Format::Json),
            _ => None,
        }
    }

    /// File extension without the dot.
    pub fn ext(self) -> &'static str {
        match self {
            Format::Csv => "csv",
            Format::Json => "json",
        }
    }
}

/// Tree, tags and active filter rows are read from.
#[derive(Clone, Copy)]
pub struct Source<'a> {
    pub tree: &'a Tree,
    pub tags: Option<&'a Tags>,
    /// When set, sizes are filtered and non-matching items are left out.
    pub filtered: Option<&'a Filtered>,
}

/// One exported item.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub path: String,
    pub kind: NodeKind,
    pub allocated: u64,
    pub logical: u64,
    /// Files below a folder; `None` for files.
    pub files: Option<u64>,
    /// Last write time as a Windows FILETIME.
    pub mtime: i64,
    pub tag: Option<Tag>,
}

impl Source<'_> {
    fn sizes(&self, id: NodeId) -> (u64, u64) {
        match self.filtered {
            Some(f) => {
                let i = id.0 as usize;
                let a = f.sizes.get(i).copied().unwrap_or(0);
                (a, f.logical.get(i).copied().unwrap_or(0))
            }
            None => {
                let n = self.tree.get(id);
                (n.allocated, n.logical)
            }
        }
    }

    fn included(&self, id: NodeId) -> bool {
        self.filtered.is_none() || self.sizes(id) != (0, 0)
    }

    fn row(&self, id: NodeId, path: String, files: Option<u64>) -> Row {
        let (allocated, logical) = self.sizes(id);
        Row {
            path,
            kind: self.tree.get(id).kind,
            allocated,
            logical,
            files,
            mtime: self.tree.times(id).mtime,
            tag: self.tags.and_then(|t| t.get(id)),
        }
    }

    /// Children kept by the filter, largest first.
    fn sorted_children(&self, id: NodeId) -> Vec<NodeId> {
        let mut kids: Vec<NodeId> = self
            .tree
            .children(id)
            .filter(|&c| self.included(c))
            .collect();
        kids.sort_by(|&a, &b| {
            self.sizes(b)
                .0
                .cmp(&self.sizes(a).0)
                .then_with(|| self.tree.name(a).cmp(self.tree.name(b)))
        });
        kids
    }

    /// Number of included files below `id`.
    fn file_count(&self, id: NodeId) -> u64 {
        let mut count = 0;
        let mut stack = vec![id];
        while let Some(n) = stack.pop() {
            for c in self.tree.children(n) {
                if self.tree.get(c).kind == NodeKind::File {
                    count += u64::from(self.included(c));
                } else {
                    stack.push(c);
                }
            }
        }
        count
    }

    fn files_of(&self, id: NodeId) -> Option<u64> {
        (self.tree.get(id).kind != NodeKind::File).then(|| self.file_count(id))
    }

    /// Rows of `root` and its descendants down to `depth` levels below it (`None`: no limit), in pre-order.
    pub fn subtree(&self, root: NodeId, depth: Option<u32>) -> Vec<Row> {
        let mut out = Vec::new();
        self.walk(root, self.tree.full_path(root), 0, depth, &mut out);
        out
    }

    /// Fills rows for `id`, an included node, and returns the number of included files it holds (1 for a file).
    fn walk(
        &self,
        id: NodeId,
        path: String,
        level: u32,
        depth: Option<u32>,
        out: &mut Vec<Row>,
    ) -> u64 {
        let is_file = self.tree.get(id).kind == NodeKind::File;
        if depth.is_some_and(|d| level > d) {
            return if is_file { 1 } else { self.file_count(id) };
        }
        let at = out.len();
        out.push(self.row(id, String::new(), None));
        let mut files = 0;
        if !is_file {
            for c in self.sorted_children(id) {
                let child = join(&path, self.tree.name(c));
                files += self.walk(c, child, level + 1, depth, out);
            }
        }
        out[at].path = path;
        if is_file {
            return 1;
        }
        out[at].files = Some(files);
        files
    }

    /// One row per node, in the given order.
    pub fn list(&self, ids: &[NodeId]) -> Vec<Row> {
        ids.iter()
            .map(|&id| self.row(id, self.tree.full_path(id), self.files_of(id)))
            .collect()
    }
}

fn join(parent: &str, name: &str) -> String {
    let mut s = String::with_capacity(parent.len() + name.len() + 1);
    s.push_str(parent);
    if !parent.ends_with('\\') {
        s.push('\\');
    }
    s.push_str(name);
    s
}

fn kind_name(kind: NodeKind) -> &'static str {
    match kind {
        NodeKind::File => "file",
        NodeKind::Dir | NodeKind::Root => "folder",
    }
}

fn tag_name(tag: Tag) -> &'static str {
    match tag {
        Tag::Red => "red",
        Tag::Yellow => "yellow",
        Tag::Green => "green",
        Tag::Blue => "blue",
    }
}

/// `YYYY-MM-DDTHH:MM:SSZ` of a FILETIME, empty for zero or negative.
pub fn iso_utc(filetime: i64) -> String {
    if filetime <= 0 {
        return String::new();
    }
    let secs = filetime / 10_000_000 - FILETIME_UNIX_SECS;
    let (y, m, d) = civil(secs.div_euclid(86_400));
    let rem = secs.rem_euclid(86_400);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

/// Year, month and day of a count of days since 1970-01-01.
fn civil(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

/// Writes rows in the given format.
pub fn write(out: &mut impl Write, format: Format, rows: &[Row]) -> io::Result<()> {
    match format {
        Format::Csv => write_csv(out, rows),
        Format::Json => write_json(out, rows),
    }
}

/// RFC 4180 CSV with a header line, CRLF line ends and a UTF-8 byte order mark for spreadsheets.
pub fn write_csv(out: &mut impl Write, rows: &[Row]) -> io::Result<()> {
    out.write_all("\u{feff}".as_bytes())?;
    write!(out, "{CSV_HEADER}\r\n")?;
    for r in rows {
        write!(
            out,
            "{},{},{},{},{},{},{}\r\n",
            csv_field(&r.path),
            kind_name(r.kind),
            r.allocated,
            r.logical,
            r.files.map_or_else(String::new, |f| f.to_string()),
            iso_utc(r.mtime),
            r.tag.map_or("", tag_name)
        )?;
    }
    Ok(())
}

/// A field quoted when it holds a comma, quote or line break; inner quotes doubled.
pub fn csv_field(s: &str) -> std::borrow::Cow<'_, str> {
    if s.contains([',', '"', '\r', '\n']) {
        format!("\"{}\"", s.replace('"', "\"\"")).into()
    } else {
        s.into()
    }
}

/// A JSON array with one object per row.
pub fn write_json(out: &mut impl Write, rows: &[Row]) -> io::Result<()> {
    out.write_all(b"[")?;
    for (i, r) in rows.iter().enumerate() {
        let sep = if i == 0 { "\n" } else { ",\n" };
        let files = r.files.map_or_else(|| "null".into(), |f| f.to_string());
        let mtime = match iso_utc(r.mtime) {
            s if s.is_empty() => "null".to_string(),
            s => json_string(&s),
        };
        let tag = r
            .tag
            .map_or_else(|| "null".into(), |t| json_string(tag_name(t)));
        write!(
            out,
            "{sep}  {{\"path\": {}, \"kind\": \"{}\", \"allocated\": {}, \"logical\": {}, \"files\": {files}, \"modified\": {mtime}, \"tag\": {tag}}}",
            json_string(&r.path),
            kind_name(r.kind),
            r.allocated,
            r.logical,
        )?;
    }
    out.write_all(if rows.is_empty() { b"]\n" } else { b"\n]\n" })
}

/// A quoted JSON string with `"`, `\` and control characters escaped.
pub fn json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::{NewEntry, Times};

    fn add(t: &mut Tree, p: NodeId, name: &str, kind: NodeKind, size: u64) -> NodeId {
        t.add_child(
            p,
            NewEntry {
                name,
                kind,
                attrs: 0,
                logical: size,
                allocated: size.next_multiple_of(4096),
                times: Times {
                    mtime: 133_000_000_000_000_000,
                    ..Times::default()
                },
            },
        )
    }

    /// `C:\s` with `a\{b\f1, f2}` and `g, h`.
    fn fixture() -> (Tree, [NodeId; 6]) {
        let mut t = Tree::new("C:\\s");
        let r = t.root();
        let a = add(&mut t, r, "a", NodeKind::Dir, 0);
        let b = add(&mut t, a, "b", NodeKind::Dir, 0);
        let f1 = add(&mut t, b, "f1", NodeKind::File, 100);
        let f2 = add(&mut t, a, "f, \"2\"", NodeKind::File, 9000);
        let g = add(&mut t, r, "g", NodeKind::File, 50);
        let h = add(&mut t, r, "h", NodeKind::File, 0);
        (t, [a, b, f1, f2, g, h])
    }

    fn src(t: &Tree) -> Source<'_> {
        Source {
            tree: t,
            tags: None,
            filtered: None,
        }
    }

    fn paths(rows: &[Row]) -> Vec<&str> {
        rows.iter().map(|r| r.path.as_str()).collect()
    }

    #[test]
    fn subtree_order_depth_and_counts() {
        let (t, [a, ..]) = fixture();
        let all = src(&t).subtree(t.root(), None);
        assert_eq!(
            paths(&all),
            [
                "C:\\s",
                "C:\\s\\a",
                "C:\\s\\a\\f, \"2\"",
                "C:\\s\\a\\b",
                "C:\\s\\a\\b\\f1",
                "C:\\s\\g",
                "C:\\s\\h"
            ]
        );
        assert_eq!(all[0].files, Some(4));
        assert_eq!(all[1].files, Some(2));
        assert_eq!(all[2].files, None);
        let one = src(&t).subtree(t.root(), Some(1));
        assert_eq!(paths(&one), ["C:\\s", "C:\\s\\a", "C:\\s\\g", "C:\\s\\h"]);
        assert_eq!((one[0].files, one[1].files), (Some(4), Some(2)));
        let a_only = src(&t).subtree(a, Some(0));
        assert_eq!(a_only.len(), 1);
        assert_eq!((a_only[0].allocated, a_only[0].logical), (16384, 9100));
    }

    #[test]
    fn filter_and_tags() {
        let (t, [a, b, f1, f2, g, h]) = fixture();
        let mut f = Filtered {
            sizes: vec![0; t.len()],
            logical: vec![0; t.len()],
            ..Filtered::default()
        };
        for id in [t.root(), a, b, f1] {
            f.sizes[id.0 as usize] = 4096;
            f.logical[id.0 as usize] = 100;
        }
        let mut tags = Tags::new();
        tags.toggle(f1, Tag::Green);
        let s = Source {
            tree: &t,
            tags: Some(&tags),
            filtered: Some(&f),
        };
        let rows = s.subtree(t.root(), None);
        assert_eq!(
            paths(&rows),
            ["C:\\s", "C:\\s\\a", "C:\\s\\a\\b", "C:\\s\\a\\b\\f1"]
        );
        assert_eq!(rows[0].files, Some(1));
        assert_eq!(rows[3].tag, Some(Tag::Green));
        let list = s.list(&[f2, a, g, h]);
        assert_eq!(list[0].allocated, 0);
        assert_eq!(list[1].files, Some(1));
    }

    #[test]
    fn csv_output() {
        let (t, [_, _, _, f2, g, _]) = fixture();
        let mut buf = Vec::new();
        write(&mut buf, Format::Csv, &src(&t).list(&[f2, g, t.root()])).unwrap();
        let text = String::from_utf8(buf).unwrap();
        assert_eq!(
            text,
            "\u{feff}path,kind,allocated,logical,files,modified,tag\r\n\
             \"C:\\s\\a\\f, \"\"2\"\"\",file,12288,9000,,2022-06-18T04:26:40Z,\r\n\
             C:\\s\\g,file,4096,50,,2022-06-18T04:26:40Z,\r\n\
             C:\\s,folder,20480,9150,4,,\r\n"
        );
        assert_eq!(csv_field("a\nb"), "\"a\nb\"");
        assert_eq!(csv_field("plain"), "plain");
    }

    #[test]
    fn json_output() {
        let (t, [_, _, _, f2, ..]) = fixture();
        let mut tags = Tags::new();
        tags.toggle(f2, Tag::Red);
        let s = Source {
            tree: &t,
            tags: Some(&tags),
            filtered: None,
        };
        let mut buf = Vec::new();
        write(&mut buf, Format::Json, &s.list(&[f2, t.root()])).unwrap();
        assert_eq!(
            String::from_utf8(buf).unwrap(),
            "[\n  {\"path\": \"C:\\\\s\\\\a\\\\f, \\\"2\\\"\", \"kind\": \"file\", \"allocated\": 12288, \"logical\": 9000, \"files\": null, \"modified\": \"2022-06-18T04:26:40Z\", \"tag\": \"red\"},\n  \
             {\"path\": \"C:\\\\s\", \"kind\": \"folder\", \"allocated\": 20480, \"logical\": 9150, \"files\": 4, \"modified\": null, \"tag\": null}\n]\n"
        );
        let mut empty = Vec::new();
        write_json(&mut empty, &[]).unwrap();
        assert_eq!(empty, b"[]\n");
        assert_eq!(json_string("a\u{1}\t\u{e9}"), "\"a\\u0001\\t\u{e9}\"");
    }

    #[test]
    fn iso_dates() {
        assert_eq!(iso_utc(133_000_000_000_000_000), "2022-06-18T04:26:40Z");
        assert_eq!(iso_utc(116_444_736_000_000_000), "1970-01-01T00:00:00Z");
        assert_eq!(iso_utc(0), "");
    }
}
