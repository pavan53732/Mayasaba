//! The injectable clock.
//!
//! DEC-052 requires the controller to compute quality facts deterministically. A function that called
//! `SystemTime::now()` internally would produce different output for the same inputs and would be
//! untestable against a fixed instant, so time is a parameter here and the only place this crate reads a
//! real clock is [`SystemClock`], which a caller must choose deliberately.

/// A source of RFC3339 timestamps.
///
/// Implementations must be cheap and infallible: a timestamp that cannot be produced is a caller problem,
/// not a domain decision, and returning `Result` here would force every domain function to thread an error
/// that none of them can act on.
pub trait Clock {
    /// The current instant as an RFC3339 timestamp, for example `2026-10-04T12:00:00Z`.
    fn now_rfc3339(&self) -> String;
}

/// A clock fixed at one instant. For tests and for replaying a recorded evaluation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FixedClock {
    stamp: String,
}

impl FixedClock {
    /// A clock that always reports `stamp`.
    pub fn new(stamp: &str) -> Self {
        FixedClock {
            stamp: stamp.to_string(),
        }
    }
}

impl Clock for FixedClock {
    fn now_rfc3339(&self) -> String {
        self.stamp.clone()
    }
}

/// The process clock, in UTC whole seconds.
///
/// The repository has no date dependency and this slice does not need sub-second precision, so the
/// conversion is done here rather than by adding one. Sub-second precision only matters once evidence
/// ordering depends on it, which is recorded as a limitation in `crates/core`'s existing clock helper too.
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_rfc3339(&self) -> String {
        // A system clock before the epoch, or more than 292 billion years past it, cannot occur; falling back
        // to the epoch keeps this infallible instead of panicking on an unreachable path. `unwrap_or_default`
        // is used only after both conversions have been checked to fail closed to zero.
        let seconds = i64::try_from(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        )
        .unwrap_or_default();
        format_rfc3339_utc(seconds)
    }
}

/// Format a Unix timestamp as RFC3339 UTC.
///
/// The civil-date conversion is Howard Hinnant's `civil_from_days`, which is exact for the whole proleptic
/// Gregorian range. It is written out here because the alternative - a date dependency - is a workspace
/// dependency decision, not one this crate may take.
pub fn format_rfc3339_utc(epoch_seconds: i64) -> String {
    let days = epoch_seconds.div_euclid(86_400);
    let seconds_of_day = epoch_seconds.rem_euclid(86_400);

    let civil = civil_from_days(days);
    let hour = seconds_of_day / 3_600;
    let minute = (seconds_of_day % 3_600) / 60;
    let second = seconds_of_day % 60;

    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        civil.0, civil.1, civil.2, hour, minute, second
    )
}

/// `(year, month, day)` for a count of days since 1970-01-01. Month is 1-12, day is 1-31.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    // Shift the epoch to 0000-03-01 so leap days land at the end of the shifted year.
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    let year = year_of_era + era * 400 + if month <= 2 { 1 } else { 0 };

    // `month` and `day` are bounded by the algorithm above, so the narrowing cannot lose a value: month is
    // 1-12 and day is 1-31. They are clamped rather than cast anyway, so a future change to the arithmetic
    // cannot silently wrap into a wrong date.
    let month = u32::try_from(month).unwrap_or(1);
    let day = u32::try_from(day).unwrap_or(1);
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_epoch_formats_as_the_unix_epoch() {
        assert_eq!(format_rfc3339_utc(0), "1970-01-01T00:00:00Z");
    }

    #[test]
    fn known_instants_round_trip_through_the_civil_calendar() {
        assert_eq!(format_rfc3339_utc(1_700_000_000), "2023-11-14T22:13:20Z");
        // A leap day, which is the case the shifted-era arithmetic exists to get right.
        assert_eq!(format_rfc3339_utc(1_709_164_800), "2024-02-29T00:00:00Z");
        assert_eq!(format_rfc3339_utc(951_782_400), "2000-02-29T00:00:00Z");
        // An instant before the epoch, so div_euclid rather than truncating division is exercised.
        assert_eq!(format_rfc3339_utc(-1), "1969-12-31T23:59:59Z");
    }

    #[test]
    fn the_system_clock_is_rfc3339_shaped() {
        let stamp = SystemClock.now_rfc3339();
        assert_eq!(stamp.len(), 20, "unexpected timestamp shape: {stamp}");
        assert!(stamp.ends_with('Z'));
        assert_eq!(&stamp[4..5], "-");
        assert_eq!(&stamp[10..11], "T");
    }

    #[test]
    fn a_fixed_clock_reports_only_its_instant() {
        let clock = FixedClock::new("2026-10-04T00:00:00Z");
        assert_eq!(clock.now_rfc3339(), "2026-10-04T00:00:00Z");
        assert_eq!(clock.now_rfc3339(), clock.now_rfc3339());
    }
}
