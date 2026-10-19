//! Minimal ISO 8601 timestamps for GPX and KML (no external time crate).

/// Days since 1970-01-01 of a proleptic Gregorian date.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year.rem_euclid(400);
    let month_index = (month + 9) % 12;
    let day_of_year = (153 * month_index + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let days = days + 719_468;
    let era = days.div_euclid(146_097);
    let day_of_era = days.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    (year_of_era + era * 400 + i64::from(month <= 2), month, day)
}

fn number(text: &str) -> Option<i64> {
    if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

/// Parses `YYYY-MM-DDThh:mm:ss[.fff](Z|+hh:mm|-hh:mm)` into seconds since the Unix epoch.
///
/// A timestamp without a zone designator is ambiguous (local time of an unknown zone), so it is
/// rejected rather than guessed: statistics that need it are then omitted.
#[must_use]
pub(crate) fn parse_timestamp(text: &str) -> Option<f64> {
    let text = text.trim();
    let (date, rest) = text.split_once(['T', ' '])?;
    let mut date_parts = date.split('-');
    let year = number(date_parts.next()?)?;
    let month = number(date_parts.next()?)?;
    let day = number(date_parts.next()?)?;
    if date_parts.next().is_some() || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let zone_at = rest.find(['Z', 'z', '+', '-'])?;
    let (clock, zone) = rest.split_at(zone_at);
    let offset_seconds = match zone {
        "Z" | "z" => 0,
        _ => {
            let sign = if zone.starts_with('-') { -1 } else { 1 };
            let digits = &zone[1..];
            let (hours, minutes) = match digits.split_once(':') {
                Some((hours, minutes)) => (number(hours)?, number(minutes)?),
                None if digits.len() == 4 => (number(&digits[..2])?, number(&digits[2..])?),
                None if digits.len() == 2 => (number(digits)?, 0),
                None => return None,
            };
            sign * (hours * 3600 + minutes * 60)
        }
    };
    let mut clock_parts = clock.split(':');
    let hour = number(clock_parts.next()?)?;
    let minute = number(clock_parts.next()?)?;
    let seconds_text = clock_parts.next()?;
    if clock_parts.next().is_some() || hour > 23 || minute > 59 {
        return None;
    }
    let (whole, fraction) = match seconds_text.split_once('.') {
        Some((whole, fraction)) => (number(whole)?, format!("0.{fraction}").parse::<f64>().ok()?),
        None => (number(seconds_text)?, 0.0),
    };
    if whole > 60 {
        return None;
    }
    let days = days_from_civil(year, month, day);
    #[allow(clippy::cast_precision_loss)]
    let seconds = (days * 86_400 + hour * 3600 + minute * 60 + whole - offset_seconds) as f64;
    Some(seconds + fraction)
}

/// Formats Unix seconds as `YYYY-MM-DDThh:mm:ssZ` (UTC).
#[must_use]
pub(crate) fn format_timestamp(seconds: f64) -> String {
    #[allow(clippy::cast_possible_truncation)]
    let whole = seconds.floor() as i64;
    let (year, month, day) = civil_from_days(whole.div_euclid(86_400));
    let rest = whole.rem_euclid(86_400);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_utc_and_offset_timestamps_to_the_same_instant() {
        let utc = parse_timestamp("2024-03-05T14:07:09Z").expect("utc");
        let offset = parse_timestamp("2024-03-05T15:07:09+01:00").expect("offset");
        assert!((utc - offset).abs() < f64::EPSILON);
        assert!((utc - 1_709_647_629.0).abs() < f64::EPSILON);
    }

    #[test]
    fn keeps_fractional_seconds_and_handles_negative_offsets() {
        let value = parse_timestamp("2024-03-05T14:07:09.250-0500").expect("parsed");
        let utc = parse_timestamp("2024-03-05T19:07:09Z").expect("utc");
        assert!((value - utc - 0.25).abs() < 1e-9);
    }

    #[test]
    fn rejects_ambiguous_or_malformed_timestamps() {
        assert_eq!(parse_timestamp("2024-03-05T14:07:09"), None);
        assert_eq!(parse_timestamp("yesterday"), None);
        assert_eq!(parse_timestamp("2024-13-05T14:07:09Z"), None);
        assert_eq!(parse_timestamp("2024-03-05T25:07:09Z"), None);
        assert_eq!(parse_timestamp(""), None);
    }

    #[test]
    fn formats_round_trip() {
        let seconds = parse_timestamp("2023-12-31T23:59:58Z").expect("parsed");
        assert_eq!(format_timestamp(seconds), "2023-12-31T23:59:58Z");
        assert_eq!(format_timestamp(0.0), "1970-01-01T00:00:00Z");
    }

    #[test]
    fn leap_days_are_counted() {
        let before = parse_timestamp("2024-02-28T00:00:00Z").expect("a");
        let after = parse_timestamp("2024-03-01T00:00:00Z").expect("b");
        assert!((after - before - 2.0 * 86_400.0).abs() < f64::EPSILON);
    }
}
