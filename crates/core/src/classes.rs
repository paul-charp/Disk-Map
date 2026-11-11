//! File classes: named groups of extensions with a display colour.

use std::collections::HashMap;

/// Colour of files that belong to no class.
pub const OTHER_COLOR: [u8; 3] = [0x49, 0x50, 0x57];

/// A named group of file extensions.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileClass {
    pub name: String,
    pub color: [u8; 3],
    pub extensions: Vec<String>,
}

/// Ordered classes with an extension lookup.
#[derive(Clone, Debug)]
pub struct FileClasses {
    classes: Vec<FileClass>,
    lookup: HashMap<String, usize>,
}

const MAX_EXT: usize = 16;

impl FileClasses {
    /// Builds the lookup; the first class listing an extension wins.
    pub fn new(mut classes: Vec<FileClass>) -> Self {
        let mut lookup = HashMap::new();
        for (i, class) in classes.iter_mut().enumerate() {
            for ext in &mut class.extensions {
                *ext = ext.trim_start_matches('.').to_lowercase();
                lookup.entry(ext.clone()).or_insert(i);
            }
        }
        FileClasses { classes, lookup }
    }

    /// The classes in order.
    pub fn classes(&self) -> &[FileClass] {
        &self.classes
    }

    /// Class of a file name, from its extension.
    pub fn class_of(&self, file_name: &str) -> Option<usize> {
        let (_, ext) = file_name.rsplit_once('.')?;
        if ext.is_ascii() && ext.len() <= MAX_EXT {
            let mut buf = [0u8; MAX_EXT];
            let buf = &mut buf[..ext.len()];
            buf.copy_from_slice(ext.as_bytes());
            buf.make_ascii_lowercase();
            self.lookup.get(std::str::from_utf8(buf).ok()?).copied()
        } else {
            self.lookup.get(&ext.to_lowercase()).copied()
        }
    }

    /// Index of a class by case-insensitive name.
    pub fn by_name(&self, name: &str) -> Option<usize> {
        let name = name.to_lowercase();
        self.classes
            .iter()
            .position(|c| c.name.to_lowercase() == name)
    }

    /// Extensions of a class.
    pub fn extensions_of(&self, index: usize) -> &[String] {
        &self.classes[index].extensions
    }
}

fn class(name: &str, color: [u8; 3], exts: &[&str]) -> FileClass {
    FileClass {
        name: name.to_string(),
        color,
        extensions: exts.iter().map(|e| e.to_string()).collect(),
    }
}

impl Default for FileClasses {
    fn default() -> Self {
        FileClasses::new(vec![
            class(
                "Video",
                [0x3B, 0x5B, 0xDB],
                &[
                    "mp4", "mkv", "avi", "mov", "wmv", "flv", "webm", "m4v", "mpg", "mpeg", "ts",
                    "m2ts", "vob", "3gp",
                ],
            ),
            class(
                "Images",
                [0x0B, 0x72, 0x85],
                &[
                    "jpg", "jpeg", "png", "gif", "bmp", "tif", "tiff", "webp", "heic", "svg",
                    "ico", "psd", "exr", "hdr", "tga", "raw", "cr2", "nef", "dng",
                ],
            ),
            class(
                "Archives",
                [0xB4, 0x53, 0x09],
                &[
                    "7z", "zip", "rar", "tar", "gz", "bz2", "xz", "zst", "iso", "cab", "tgz",
                    "wim", "img",
                ],
            ),
            class(
                "Code",
                [0x67, 0x41, 0xD9],
                &[
                    "rs", "py", "c", "h", "cpp", "hpp", "cs", "js", "jsx", "tsx", "java", "go",
                    "lua", "rb", "php", "sh", "bat", "ps1", "html", "css", "json", "xml", "yaml",
                    "yml", "toml", "vex", "usda", "glsl", "hlsl",
                ],
            ),
            class(
                "Documents",
                [0x23, 0x70, 0x32],
                &[
                    "pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "odt", "ods", "txt", "md",
                    "rtf", "csv", "epub", "log",
                ],
            ),
            class(
                "Audio",
                [0xA6, 0x1E, 0x4D],
                &[
                    "mp3", "wav", "flac", "ogg", "aac", "m4a", "wma", "opus", "aiff", "mid",
                ],
            ),
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_lookup() {
        let c = FileClasses::default();
        let name = |n: &str| c.class_of(n).map(|i| c.classes()[i].name.as_str());
        assert_eq!(name("movie.MKV"), Some("Video"));
        assert_eq!(name("a.b.JPG"), Some("Images"));
        assert_eq!(name("x.7z"), Some("Archives"));
        assert_eq!(name("main.rs"), Some("Code"));
        assert_eq!(name("a.pdf"), Some("Documents"));
        assert_eq!(name("a.flac"), Some("Audio"));
        assert_eq!(name("noext"), None);
        assert_eq!(name("a.unknownextension12345"), None);
        assert_eq!(name("a.zzz"), None);
    }

    #[test]
    fn by_name_and_colors() {
        let c = FileClasses::default();
        let i = c.by_name("aUdIo").unwrap();
        assert_eq!(c.classes()[i].color, [0xA6, 0x1E, 0x4D]);
        assert!(c.extensions_of(i).iter().any(|e| e == "mp3"));
        assert_eq!(c.by_name("nope"), None);
        let a = c.by_name("archives").unwrap();
        for ext in ["7z", "zip", "rar", "tar", "gz", "iso"] {
            assert!(c.extensions_of(a).iter().any(|e| e == ext));
        }
        assert_eq!(OTHER_COLOR, [0x49, 0x50, 0x57]);
    }

    #[test]
    fn first_class_wins_and_normalises() {
        let c = FileClasses::new(vec![
            class("One", [0; 3], &[".Foo"]),
            class("Two", [0; 3], &["foo", "bar"]),
        ]);
        assert_eq!(c.class_of("x.FOO"), Some(0));
        assert_eq!(c.class_of("x.bar"), Some(1));
    }
}
