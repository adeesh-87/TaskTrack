//! Tiny UTC time helpers (no date crate needed).

use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

/// Seconds since the Unix epoch.
pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Current time as `YYYY-MM-DDTHH:MM:SSZ`.
pub fn now_rfc3339() -> String {
    format_rfc3339(now_secs())
}

/// Format seconds since the Unix epoch as RFC 3339 UTC.
pub fn format_rfc3339(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    // Howard Hinnant's civil-from-days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

/// `2026-09-24T10:00:00Z` → `2026-09-24 15:30` in local time (for logs).
pub fn short(ts: &str) -> String {
    short_with_offset(ts, local_offset_secs())
}

/// [`short`] with an explicit UTC offset in seconds.
pub fn short_with_offset(ts: &str, offset: i64) -> String {
    match parse_rfc3339(ts) {
        Some(secs) => {
            let local = format_rfc3339((secs as i64 + offset).max(0) as u64);
            format!("{} {}", &local[..10], &local[11..16])
        }
        None => ts.to_owned(),
    }
}

/// The local UTC offset in seconds (from `date +%z`, cached; 0 if unknown).
pub fn local_offset_secs() -> i64 {
    static OFFSET: OnceLock<i64> = OnceLock::new();
    *OFFSET.get_or_init(|| {
        std::process::Command::new("date")
            .arg("+%z")
            .output()
            .ok()
            .and_then(|o| parse_offset(String::from_utf8_lossy(&o.stdout).trim()))
            .unwrap_or(0)
    })
}

/// `+0530` → 19800, `-0700` → -25200.
pub fn parse_offset(s: &str) -> Option<i64> {
    let (sign, rest) = match s.as_bytes().first()? {
        b'+' => (1, &s[1..]),
        b'-' => (-1, &s[1..]),
        _ => return None,
    };
    if rest.len() != 4 || !rest.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let h: i64 = rest[..2].parse().ok()?;
    let m: i64 = rest[2..].parse().ok()?;
    Some(sign * (h * 3600 + m * 60))
}

/// Days since the epoch of a civil date (Howard Hinnant's days-from-civil).
pub fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Parse `YYYY-MM-DDTHH:MM:SSZ` (or just `YYYY-MM-DD`) into epoch seconds.
pub fn parse_rfc3339(ts: &str) -> Option<u64> {
    let b = ts.as_bytes();
    if b.len() < 10 || b[4] != b'-' || b[7] != b'-' {
        return None;
    }
    let num = |r: std::ops::Range<usize>| ts.get(r)?.parse::<i64>().ok();
    let days = days_from_civil(num(0..4)?, num(5..7)?, num(8..10)?);
    let (h, mi, s) = if b.len() >= 19 && b[10] == b'T' {
        (num(11..13)?, num(14..16)?, num(17..19)?)
    } else {
        (0, 0, 0)
    };
    u64::try_from(days * 86_400 + h * 3600 + mi * 60 + s).ok()
}

/// Parse the dates ticket and review tools print into epoch seconds (UTC):
/// `2026-09-21`, `2026-09-21T09:30:00Z`, `2026-09-21T09:30:00.000+0200`,
/// `2026-09-21 09:30:00.000000000` (Gerrit, UTC), `…+02:00`, `… UTC`,
/// and plain epoch seconds or milliseconds.
pub fn parse_loose(ts: &str) -> Option<u64> {
    let t = ts.trim();
    if !t.is_empty() && t.bytes().all(|b| b.is_ascii_digit()) {
        let n: u64 = t.parse().ok()?;
        return Some(if t.len() >= 13 { n / 1000 } else { n });
    }
    let b = t.as_bytes();
    if b.len() < 10 || b[4] != b'-' || b[7] != b'-' {
        return None;
    }
    let num = |s: &str| s.parse::<i64>().ok();
    let days = days_from_civil(num(t.get(0..4)?)?, num(t.get(5..7)?)?, num(t.get(8..10)?)?);
    let mut rest = &t[10..];
    let mut secs = 0i64;
    if let Some(r) = rest.strip_prefix(['T', ' ']) {
        let time_len = r
            .bytes()
            .take_while(|c| c.is_ascii_digit() || *c == b':')
            .count();
        let parts: Vec<i64> = r[..time_len]
            .split(':')
            .map(num)
            .collect::<Option<Vec<_>>>()?;
        let (h, m, s) = match parts[..] {
            [h, m] => (h, m, 0),
            [h, m, s] => (h, m, s),
            _ => return None,
        };
        secs = h * 3600 + m * 60 + s;
        rest = &r[time_len..];
        if let Some(frac) = rest.strip_prefix('.') {
            rest = frac.trim_start_matches(|c: char| c.is_ascii_digit());
        }
    }
    let zone = rest.trim();
    let offset = match zone {
        "" | "Z" | "z" | "UTC" | "GMT" => 0,
        z => {
            let sign = match z.as_bytes()[0] {
                b'+' => 1,
                b'-' => -1,
                _ => return None,
            };
            let digits: String = z[1..].chars().filter(char::is_ascii_digit).collect();
            let (h, m) = match digits.len() {
                2 => (num(&digits)?, 0),
                4 => (num(&digits[..2])?, num(&digits[2..])?),
                _ => return None,
            };
            sign * (h * 3600 + m * 60)
        }
    };
    u64::try_from(days * 86_400 + secs - offset).ok()
}

/// A date as tools print it (see [`parse_loose`]) → RFC 3339 UTC.
pub fn normalize(ts: &str) -> Option<String> {
    parse_loose(ts).map(format_rfc3339)
}

/// Epoch seconds of the start of the local day containing `secs`.
pub fn local_day_start(secs: u64, offset: i64) -> u64 {
    let local = secs as i64 + offset;
    (local.div_euclid(86_400) * 86_400 - offset).max(0) as u64
}

/// Epoch seconds of the start of the local week (Monday) containing `secs`.
pub fn local_week_start(secs: u64, offset: i64) -> u64 {
    let local = secs as i64 + offset;
    let days = local.div_euclid(86_400);
    let monday = days - (days + 3).rem_euclid(7);
    (monday * 86_400 - offset).max(0) as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loose_dates() {
        let z = |s: &str| normalize(s).unwrap_or_else(|| panic!("{s}"));
        assert_eq!(z("2026-09-21"), "2026-09-21T00:00:00Z");
        assert_eq!(z("2026-09-21T09:30:00Z"), "2026-09-21T09:30:00Z");
        assert_eq!(z("2026-09-21T09:30:00.000+0200"), "2026-09-21T07:30:00Z");
        assert_eq!(z("2026-09-21T09:30:00-05:30"), "2026-09-21T15:00:00Z");
        assert_eq!(z("2026-09-21 09:30:00.000000000"), "2026-09-21T09:30:00Z");
        assert_eq!(z("2026-09-21 09:30 UTC"), "2026-09-21T09:30:00Z");
        assert_eq!(z("1790000000"), "2026-09-21T14:13:20Z");
        assert_eq!(z("1790000000123"), "2026-09-21T14:13:20Z");
        assert_eq!(normalize("yesterday"), None);
        assert_eq!(normalize("2026-09-21T09"), None);
        assert_eq!(normalize("2026-09-21T09:30 CEST"), None);
    }

    #[test]
    fn formats() {
        assert_eq!(format_rfc3339(0), "1970-01-01T00:00:00Z");
        assert_eq!(format_rfc3339(1_700_000_000), "2023-11-14T22:13:20Z");
        assert_eq!(format_rfc3339(1_709_164_800), "2024-02-29T00:00:00Z");
        assert_eq!(
            short_with_offset("2026-09-24T10:05:33Z", 0),
            "2026-09-24 10:05"
        );
        assert_eq!(
            short_with_offset("2026-09-24T20:05:33Z", 19_800),
            "2026-09-25 01:35"
        );
        assert_eq!(short("x"), "x");
        assert_eq!(parse_offset("+0530"), Some(19_800));
        assert_eq!(parse_offset("-0700"), Some(-25_200));
        assert_eq!(parse_offset("x"), None);
        for secs in [0, 1_700_000_000, 1_709_164_800, 1_790_000_123] {
            assert_eq!(parse_rfc3339(&format_rfc3339(secs)), Some(secs));
        }
        assert_eq!(
            parse_rfc3339("2026-01-02"),
            parse_rfc3339("2026-01-02T00:00:00Z")
        );
        assert_eq!(parse_rfc3339("nope"), None);
        // 2026-09-24 is a Thursday.
        let t = parse_rfc3339("2026-09-24T10:00:00Z").unwrap();
        assert_eq!(
            format_rfc3339(local_day_start(t, 0)),
            "2026-09-24T00:00:00Z"
        );
        assert_eq!(
            format_rfc3339(local_week_start(t, 0)),
            "2026-09-21T00:00:00Z"
        );
        assert_eq!(
            format_rfc3339(local_day_start(t, 19_800)),
            "2026-09-23T18:30:00Z"
        );
    }
}
