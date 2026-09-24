//! Tiny UTC time helpers (no date crate needed).

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

/// `2026-09-24T10:00:00Z` → `2026-09-24 10:00` (short, for logs).
pub fn short(ts: &str) -> String {
    match ts.split_once('T') {
        Some((d, t)) => format!("{d} {}", t.get(..5).unwrap_or(t)),
        None => ts.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats() {
        assert_eq!(format_rfc3339(0), "1970-01-01T00:00:00Z");
        assert_eq!(format_rfc3339(1_700_000_000), "2023-11-14T22:13:20Z");
        assert_eq!(format_rfc3339(1_709_164_800), "2024-02-29T00:00:00Z");
        assert_eq!(short("2026-09-24T10:05:33Z"), "2026-09-24 10:05");
        assert_eq!(short("x"), "x");
    }
}
