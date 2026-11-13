//! Human-readable sizes and dates.

const UNITS: [&str; 6] = ["B", "KB", "MB", "GB", "TB", "PB"];
const FILETIME_UNIX_SECS: i64 = 11_644_473_600;

/// Binary-unit size with three significant digits, e.g. `38.2 GB`.
pub fn size(bytes: u64) -> String {
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut v = bytes as f64;
    let mut unit = 0;
    while v >= 1024.0 && unit < UNITS.len() - 1 {
        v /= 1024.0;
        unit += 1;
    }
    let text = if v < 9.995 {
        format!("{v:.2}")
    } else if v < 99.95 {
        format!("{v:.1}")
    } else {
        format!("{v:.0}")
    };
    let text = if text.contains('.') {
        text.trim_end_matches('0').trim_end_matches('.')
    } else {
        &text
    };
    format!("{text} {}", UNITS[unit])
}

/// Size change with its sign, e.g. `+4.2 GB` or `-512 MB`.
pub fn signed_size(delta: i64) -> String {
    let sign = if delta < 0 { '-' } else { '+' };
    format!("{sign}{}", size(delta.unsigned_abs()))
}

/// Percentage with one decimal below 10, none above.
pub fn percent(part: u64, total: u64) -> String {
    if total == 0 {
        return "0%".into();
    }
    let p = part as f64 * 100.0 / total as f64;
    if p < 10.0 {
        format!("{p:.1}%")
    } else {
        format!("{p:.0}%")
    }
}

/// Current time as a Windows FILETIME.
pub fn filetime_now() -> i64 {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64);
    (secs + FILETIME_UNIX_SECS) * 10_000_000
}

/// `YYYY-MM-DD HH:MM` (UTC) of a FILETIME, empty for zero.
pub fn date_time(filetime: i64) -> String {
    if filetime <= 0 {
        return String::new();
    }
    let secs = filetime / 10_000_000 - FILETIME_UNIX_SECS;
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02} {:02}:{:02}",
        rem / 3600,
        rem % 3600 / 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes() {
        assert_eq!(size(512), "512 B");
        assert_eq!(size(512 * 1024 * 1024), "512 MB");
        assert_eq!(size(41_011_000_000), "38.2 GB");
        assert_eq!(size(2_089_000_000_000), "1.9 TB");
        assert_eq!(signed_size(-512 * 1024 * 1024), "-512 MB");
        assert_eq!(signed_size(0), "+0 B");
    }

    #[test]
    fn dates() {
        assert_eq!(date_time(133_000_000_000_000_000), "2022-06-18 04:26");
        assert_eq!(date_time(0), "");
    }
}
