//! File age buckets, used to colour files by modification time.

const TICKS_PER_DAY: i64 = 864_000_000_000;

/// Upper bound in days of every bucket but the last: a week, a month, six months, a year, three years.
const LIMIT_DAYS: [i64; 5] = [7, 30, 182, 365, 1095];

/// Number of age buckets; the last holds everything older, and unknown times.
pub const AGE_BUCKETS: usize = LIMIT_DAYS.len() + 1;

/// Bucket of a modification time relative to `now`, both Windows FILETIME ticks; 0 is the most recent.
pub fn age_bucket(mtime: i64, now: i64) -> u8 {
    if mtime <= 0 {
        return (AGE_BUCKETS - 1) as u8;
    }
    let age = now.saturating_sub(mtime);
    LIMIT_DAYS
        .iter()
        .position(|&d| age < d * TICKS_PER_DAY)
        .unwrap_or(AGE_BUCKETS - 1) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 134_000_000_000_000_000;

    fn days_ago(d: i64) -> u8 {
        age_bucket(NOW - d * TICKS_PER_DAY, NOW)
    }

    #[test]
    fn buckets_by_age() {
        assert_eq!(days_ago(0), 0);
        assert_eq!(days_ago(6), 0);
        assert_eq!(days_ago(7), 1);
        assert_eq!(days_ago(29), 1);
        assert_eq!(days_ago(100), 2);
        assert_eq!(days_ago(300), 3);
        assert_eq!(days_ago(1000), 4);
        assert_eq!(days_ago(1095), 5);
        assert_eq!(days_ago(10_000), 5);
    }

    #[test]
    fn future_and_unknown_times() {
        assert_eq!(days_ago(-3), 0);
        assert_eq!(age_bucket(0, NOW), (AGE_BUCKETS - 1) as u8);
    }
}
