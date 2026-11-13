//! Timeline retention and point names, on UTC Unix seconds.

use std::collections::HashMap;

const DAY: i64 = 86_400;
const FILETIME_UNIX_SECS: i64 = 11_644_473_600;
const FILETIME_PER_SEC: i64 = 10_000_000;

/// Points younger than this many days are kept one per day.
pub const DAILY_DAYS: i64 = 90;
/// Points younger than this many days (and older than [`DAILY_DAYS`]) are kept one per ISO week.
pub const WEEKLY_DAYS: i64 = 365;

/// Unix seconds of a FILETIME.
pub fn unix_from_filetime(ft: i64) -> i64 {
    ft.div_euclid(FILETIME_PER_SEC) - FILETIME_UNIX_SECS
}

/// FILETIME of Unix seconds.
pub fn filetime_from_unix(secs: i64) -> i64 {
    (secs + FILETIME_UNIX_SECS) * FILETIME_PER_SEC
}

/// Year, month and day of a day number counted from 1970-01-01.
pub fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

/// Day number of a date, counted from 1970-01-01.
pub fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let m = i64::from(m);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + i64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// ISO 8601 year and week of a day number.
pub fn iso_week(days: i64) -> (i64, u32) {
    let monday_based = (days + 3).rem_euclid(7);
    let thursday = days - monday_based + 3;
    let year = civil(thursday).0;
    let week = (thursday - days_from_civil(year, 1, 1)) / 7 + 1;
    (year, week as u32)
}

/// `YYYYMMDD-HHMMSS` of Unix seconds (UTC).
pub fn stamp(secs: i64) -> String {
    let (y, m, d) = civil(secs.div_euclid(DAY));
    let rem = secs.rem_euclid(DAY);
    format!(
        "{y:04}{m:02}{d:02}-{:02}{:02}{:02}",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

/// Unix seconds of a [`stamp`].
pub fn parse_stamp(s: &str) -> Option<i64> {
    let (date, time) = s.split_once('-')?;
    if date.len() != 8 || time.len() != 6 || !s.bytes().all(|b| b.is_ascii_digit() || b == b'-') {
        return None;
    }
    let n = |t: &str| t.parse::<u32>().ok();
    let (y, m, d) = (n(&date[..4])?, n(&date[4..6])?, n(&date[6..])?);
    let (hh, mm, ss) = (n(&time[..2])?, n(&time[2..4])?, n(&time[4..])?);
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) || hh > 23 || mm > 59 || ss > 59 {
        return None;
    }
    let days = days_from_civil(i64::from(y), m, d);
    Some(days * DAY + i64::from(hh * 3600 + mm * 60 + ss))
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Bucket {
    Day(i64),
    Week(i64, u32),
    Month(i64, u32),
}

fn bucket(secs: i64, now: i64) -> Bucket {
    let day = secs.div_euclid(DAY);
    let age = now.div_euclid(DAY) - day;
    if age < DAILY_DAYS {
        Bucket::Day(day)
    } else if age < WEEKLY_DAYS {
        let (y, w) = iso_week(day);
        Bucket::Week(y, w)
    } else {
        let (y, m, _) = civil(day);
        Bucket::Month(y, m)
    }
}

/// Which points to keep, by position in `times` (Unix seconds): the last point of each day for
/// [`DAILY_DAYS`], then the first of each ISO week until [`WEEKLY_DAYS`], then the first of each month.
pub fn keep(times: &[i64], now: i64) -> Vec<bool> {
    let mut order: Vec<usize> = (0..times.len()).collect();
    order.sort_by_key(|&i| times[i]);
    let mut chosen: HashMap<Bucket, usize> = HashMap::new();
    for i in order {
        let b = bucket(times[i], now);
        match b {
            Bucket::Day(_) => {
                chosen.insert(b, i);
            }
            _ => {
                chosen.entry(b).or_insert(i);
            }
        }
    }
    let mut out = vec![false; times.len()];
    for i in chosen.into_values() {
        out[i] = true;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(y: i64, m: u32, d: u32, hour: i64) -> i64 {
        days_from_civil(y, m, d) * DAY + hour * 3600
    }

    #[test]
    fn calendar_round_trips() {
        for days in [-1000, 0, 59, 365, 11_016, 20_000, 30_000] {
            let (y, m, d) = civil(days);
            assert_eq!(days_from_civil(y, m, d), days);
        }
        assert_eq!(civil(0), (1970, 1, 1));
        assert_eq!(
            days_from_civil(2024, 2, 29) + 1,
            days_from_civil(2024, 3, 1)
        );
    }

    #[test]
    fn iso_weeks() {
        assert_eq!(iso_week(days_from_civil(2021, 1, 3)), (2020, 53));
        assert_eq!(iso_week(days_from_civil(2021, 1, 4)), (2021, 1));
        assert_eq!(iso_week(days_from_civil(2024, 12, 30)), (2025, 1));
        assert_eq!(iso_week(days_from_civil(2026, 11, 12)), (2026, 46));
    }

    #[test]
    fn stamps() {
        let t = at(2026, 11, 12, 3) + 4 * 60 + 5;
        assert_eq!(stamp(t), "20261112-030405");
        assert_eq!(parse_stamp("20261112-030405"), Some(t));
        assert_eq!(parse_stamp("20261312-030405"), None);
        assert_eq!(parse_stamp("2026111-0304050"), None);
        assert_eq!(parse_stamp("+0261112-030405"), None);
        let ft = filetime_from_unix(t);
        assert_eq!(unix_from_filetime(ft), t);
    }

    #[test]
    fn recent_points_keep_the_last_of_each_day() {
        let now = at(2026, 11, 12, 12);
        let times = [
            at(2026, 11, 12, 1),
            at(2026, 11, 12, 9),
            at(2026, 11, 11, 23),
            at(2026, 11, 11, 3),
        ];
        assert_eq!(keep(&times, now), [false, true, true, false]);
    }

    #[test]
    fn older_points_thin_to_weeks_then_months() {
        let now = at(2026, 11, 12, 12);
        // 2026-06-01 is a Monday, about 164 days back.
        let week: Vec<i64> = (1..=7).map(|d| at(2026, 6, d, 3)).collect();
        let kept = keep(&week, now);
        assert_eq!(kept, [true, false, false, false, false, false, false]);
        let month: Vec<i64> = [3, 10, 17, 24].iter().map(|&d| at(2025, 3, d, 3)).collect();
        assert_eq!(keep(&month, now), [true, false, false, false]);
    }

    #[test]
    fn daily_series_over_two_years() {
        let now = at(2026, 11, 12, 12);
        let first = days_from_civil(2024, 11, 12);
        let times: Vec<i64> = (first..=first + 730).map(|d| d * DAY + 3 * 3600).collect();
        let kept = keep(&times, now).iter().filter(|&&k| k).count();
        // 90 daily, about 39 weekly, about 13 monthly.
        assert!((135..=150).contains(&kept), "{kept}");
        let again: Vec<i64> = times
            .iter()
            .zip(keep(&times, now))
            .filter_map(|(&t, k)| k.then_some(t))
            .collect();
        assert!(keep(&again, now).iter().all(|&k| k), "retention is stable");
    }
}
