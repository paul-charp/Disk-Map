//! Log levels, a bounded entry ring and the log file rotation rule. No I/O.

use std::collections::VecDeque;

/// Severity of a log entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Error,
    Warn,
    Info,
}

impl Level {
    /// Lowercase name: `error`, `warn` or `info`.
    pub fn name(self) -> &'static str {
        match self {
            Level::Error => "error",
            Level::Warn => "warn",
            Level::Info => "info",
        }
    }

    /// Parses a name as given by [`Level::name`]; anything else is `Info`.
    pub fn parse(s: &str) -> Level {
        match s {
            "error" => Level::Error,
            "warn" => Level::Warn,
            _ => Level::Info,
        }
    }
}

/// The last `cap` items pushed, each with a sequence number that keeps growing across evictions and clears.
pub struct Ring<T> {
    items: VecDeque<(u64, T)>,
    cap: usize,
    next: u64,
}

impl<T> Ring<T> {
    /// An empty ring holding at most `cap` items (at least one).
    pub fn new(cap: usize) -> Self {
        Ring {
            items: VecDeque::new(),
            cap: cap.max(1),
            next: 0,
        }
    }

    /// Appends an item, dropping the oldest when full; returns its sequence number.
    pub fn push(&mut self, item: T) -> u64 {
        if self.items.len() == self.cap {
            self.items.pop_front();
        }
        let seq = self.next;
        self.next += 1;
        self.items.push_back((seq, item));
        seq
    }

    /// Items with their sequence numbers, oldest first.
    pub fn iter(&self) -> impl Iterator<Item = (u64, &T)> {
        self.items.iter().map(|(s, t)| (*s, t))
    }

    /// Number of items held.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// True when nothing is held.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Drops every item; sequence numbers keep growing.
    pub fn clear(&mut self) {
        self.items.clear();
    }
}

/// True when appending `incoming` bytes to a file of `current` bytes would pass `cap`.
/// An empty file never rotates, so a single long line still gets written.
pub fn needs_rotation(current: u64, incoming: u64, cap: u64) -> bool {
    current > 0 && current.saturating_add(incoming) > cap
}

/// One log file line: `time LEVEL source: message`, continuation lines indented.
pub fn format_line(time: &str, level: Level, source: &str, message: &str) -> String {
    let message = message
        .trim_end()
        .replace("\r\n", "\n")
        .replace('\n', "\n    ");
    format!(
        "{time} {:<5} {source}: {message}\n",
        level.name().to_uppercase()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_keeps_the_newest() {
        let mut r = Ring::new(3);
        for i in 0..5 {
            assert_eq!(r.push(i), i as u64);
        }
        let held: Vec<_> = r.iter().map(|(s, &v)| (s, v)).collect();
        assert_eq!(held, [(2, 2), (3, 3), (4, 4)]);
        assert_eq!(r.len(), 3);
    }

    #[test]
    fn clear_keeps_sequence() {
        let mut r = Ring::new(2);
        r.push("a");
        r.push("b");
        r.clear();
        assert!(r.is_empty());
        assert_eq!(r.push("c"), 2);
        assert_eq!(r.iter().count(), 1);
    }

    #[test]
    fn zero_capacity_holds_one() {
        let mut r = Ring::new(0);
        r.push(1);
        r.push(2);
        assert_eq!(r.iter().map(|(_, &v)| v).collect::<Vec<_>>(), [2]);
    }

    #[test]
    fn rotation_rule() {
        assert!(!needs_rotation(0, 5000, 1000));
        assert!(!needs_rotation(900, 100, 1000));
        assert!(needs_rotation(901, 100, 1000));
        assert!(needs_rotation(u64::MAX, 1, 1000));
    }

    #[test]
    fn levels_round_trip() {
        for l in [Level::Error, Level::Warn, Level::Info] {
            assert_eq!(Level::parse(l.name()), l);
        }
        assert_eq!(Level::parse("debug"), Level::Info);
    }

    #[test]
    fn line_format() {
        assert_eq!(
            format_line("T", Level::Warn, "scan", "a\r\nb\n"),
            "T WARN  scan: a\n    b\n"
        );
        assert_eq!(format_line("T", Level::Error, "ui", "x"), "T ERROR ui: x\n");
    }
}
