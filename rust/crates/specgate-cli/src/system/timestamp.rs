//! UTC run-identifier formatting for default capture directories.
use std::time::{SystemTime, UNIX_EPOCH};

const SECONDS_PER_DAY: u64 = 86_400;

/// Format an instant as a compact UTC run id such as `20261016T153100Z`.
///
/// Instants before the Unix epoch format as the epoch.
pub(crate) fn run_id(instant: SystemTime) -> String {
    let seconds = instant.duration_since(UNIX_EPOCH).map_or(0, |elapsed| elapsed.as_secs());
    let (year, month, day) = civil_from_days(seconds / SECONDS_PER_DAY);
    let in_day = seconds % SECONDS_PER_DAY;
    let hour = in_day / 3_600;
    let minute = in_day % 3_600 / 60;
    let second = in_day % 60;
    format!("{year:04}{month:02}{day:02}T{hour:02}{minute:02}{second:02}Z")
}

/// Convert days since 1970-01-01 to a proleptic Gregorian (year, month, day).
fn civil_from_days(days: u64) -> (u64, u64, u64) {
    let shifted = days + 719_468;
    let era = shifted / 146_097;
    let day_of_era = shifted % 146_097;
    let year_of_era = (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 { month_index + 3 } else { month_index - 9 };
    let year = year_of_era + era * 400 + u64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::run_id;
    use std::time::{Duration, UNIX_EPOCH};

    #[test]
    fn formats_known_instants() {
        assert_eq!(run_id(UNIX_EPOCH), "19700101T000000Z");
        assert_eq!(run_id(UNIX_EPOCH + Duration::from_mins(29_869_411)), "20261016T153100Z");
        // Leap day and year rollover.
        assert_eq!(run_id(UNIX_EPOCH + Duration::from_secs(1_709_164_799)), "20240228T235959Z");
        assert_eq!(run_id(UNIX_EPOCH + Duration::from_hours(474_768)), "20240229T000000Z");
        assert_eq!(run_id(UNIX_EPOCH + Duration::from_secs(1_735_689_599)), "20241231T235959Z");
    }
}
