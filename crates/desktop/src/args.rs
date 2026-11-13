//! Command line: paths to open in tabs and test switches.

use crate::config::{Config, FileColors};
use serde::Serialize;

/// Test switches, applied by the front end to the active tab once its scan is done.
#[derive(Serialize, Default, Debug, PartialEq)]
pub struct Flags {
    pub select: Option<String>,
    pub zoom: Option<String>,
    pub hover: Option<String>,
    pub tooltip: Option<String>,
    pub detail: Option<u32>,
    pub side: bool,
    /// Settings section to open at startup.
    pub settings: Option<String>,
    /// Show the Output panel at startup.
    pub log: bool,
}

/// Headless snapshot run: `--snapshot <root>` (repeatable), `--quiet`, `--no-toast`.
#[derive(Default, Debug, PartialEq)]
pub struct Headless {
    pub roots: Vec<String>,
    /// No console output.
    pub quiet: bool,
    /// No toast when done.
    pub no_toast: bool,
}

/// Parsed command line.
#[derive(Default, Debug, PartialEq)]
pub struct Args {
    /// One entry per tab to open; an empty entry is a start tab.
    pub tabs: Vec<String>,
    pub flags: Flags,
    pub headless: Headless,
    theme: Option<String>,
    scale: Option<u32>,
    colors: Option<FileColors>,
}

impl Args {
    /// Parses the arguments after the program name.
    pub fn parse(args: impl IntoIterator<Item = String>) -> Args {
        let mut out = Args::default();
        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--select" => out.flags.select = args.next(),
                "--zoom" => out.flags.zoom = args.next(),
                "--hover" => out.flags.hover = args.next(),
                "--tooltip" => out.flags.tooltip = args.next(),
                "--detail" => out.flags.detail = args.next().and_then(|v| v.parse().ok()),
                "--settings" => out.flags.settings = args.next(),
                "--theme" => out.theme = args.next(),
                "--scale" => out.scale = args.next().and_then(|v| v.parse().ok()),
                "--colour" => out.colors = args.next().and_then(|v| FileColors::parse(&v)),
                "--tabs" => out.tabs.extend(
                    args.next()
                        .iter()
                        .flat_map(|v| v.split(';'))
                        .map(str::to_string),
                ),
                "--snapshot" => out.headless.roots.extend(args.next()),
                "--quiet" => out.headless.quiet = true,
                "--no-toast" => out.headless.no_toast = true,
                "--side" => out.flags.side = true,
                "--log" => out.flags.log = true,
                a if a.starts_with("--") => {}
                _ => out.tabs.push(arg),
            }
        }
        out
    }

    /// The arguments of this process.
    pub fn from_env() -> Args {
        Args::parse(std::env::args().skip(1))
    }

    /// Applies the switches that override the configuration for this run.
    pub fn apply(&self, cfg: &mut Config) {
        if let Some(theme) = &self.theme {
            cfg.theme.clone_from(theme);
        }
        if let Some(scale) = self.scale {
            cfg.ui_scale = scale;
        }
        if let Some(colors) = self.colors {
            cfg.file_colors = colors;
        }
        cfg.sanitize();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(line: &[&str]) -> Args {
        Args::parse(line.iter().map(|s| s.to_string()))
    }

    #[test]
    fn paths_and_switches() {
        let a = parse(&[
            r"C:\a", "--select", r"x\y", "--tabs", r"D:\;", "--side", "--log", "--scale", "150",
            "--colour", "age",
        ]);
        assert_eq!(a.tabs, [r"C:\a", r"D:\", ""]);
        assert_eq!(a.flags.select.as_deref(), Some(r"x\y"));
        assert!(a.flags.side && a.flags.log);
        let mut cfg = Config::default();
        a.apply(&mut cfg);
        assert_eq!(cfg.ui_scale, 150);
        assert_eq!(cfg.file_colors, FileColors::Age);
        assert!(a.headless.roots.is_empty());
    }

    #[test]
    fn headless_switches() {
        let a = parse(&[
            "--snapshot",
            r"C:\",
            "--snapshot",
            r"D:\x",
            "--quiet",
            "--no-toast",
        ]);
        assert_eq!(a.headless.roots, [r"C:\", r"D:\x"]);
        assert!(a.headless.quiet && a.headless.no_toast);
        assert!(a.tabs.is_empty());
    }
}
