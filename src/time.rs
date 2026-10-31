//! Battle times are kept as milliseconds since the Unix epoch, as the client sends them.

use std::time::{SystemTime, UNIX_EPOCH};

/// The current time in milliseconds since the Unix epoch.
pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as i64)
        .unwrap_or(0)
}

/// Formats a time as `YYYY-MM-DD HH:MM:SS` in UTC. Zero means "never" and gives an empty string.
pub fn format_ms(ms: i64) -> String {
    if ms <= 0 {
        return String::new();
    }
    let seconds = ms / 1000;
    let (year, month, day) = civil_from_days(seconds.div_euclid(86_400));
    let in_day = seconds.rem_euclid(86_400);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02}",
        in_day / 3600,
        in_day % 3600 / 60,
        in_day % 60
    )
}

/// Converts days since the Unix epoch to a calendar date (Howard Hinnant's `civil_from_days`).
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 { shifted_month + 3 } else { shifted_month - 9 };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_known_times() {
        assert_eq!(format_ms(1_000), "1970-01-01 00:00:01");
        assert_eq!(format_ms(951_782_400_000), "2000-02-29 00:00:00");
        assert_eq!(format_ms(1_793_491_199_000), "2026-10-31 23:59:59");
    }

    #[test]
    fn zero_means_never() {
        assert_eq!(format_ms(0), "");
    }
}
