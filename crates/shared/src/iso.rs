//! ISO-8601 dates and offsets, read as strictly as `java.time`'s ISO formatters read them.
//!
//! Java counterpart: `DateTimeFormatter.ISO_LOCAL_DATE` (Jackson's `LocalDate`), `ISO_DATE` (Spring's
//! `@DateTimeFormat(iso = DATE)` on query parameters) and the offset part of `ISO_INSTANT`. Only ASCII digits,
//! and only four-digit years: Java writes larger years with a sign, which no Ledgerly date has.

use chrono::NaiveDate;

/// `yyyy-MM-dd`, a real calendar date. Java: `LocalDate.parse` with `ISO_LOCAL_DATE`.
#[must_use]
pub fn parse_date(text: &str) -> Option<NaiveDate> {
    let bytes = text.as_bytes();
    let shaped = bytes.len() == 10
        && bytes.iter().enumerate().all(|(index, byte)| {
            if matches!(index, 4 | 7) {
                *byte == b'-'
            } else {
                byte.is_ascii_digit()
            }
        });
    if !shaped {
        return None;
    }
    NaiveDate::from_ymd_opt(
        text.get(0..4)?.parse().ok()?,
        text.get(5..7)?.parse().ok()?,
        text.get(8..10)?.parse().ok()?,
    )
}

/// `yyyy-MM-dd` with an optional offset after it, which is read and ignored. Java: `ISO_DATE`, so
/// `2026-01-31Z` and `2026-01-31+01:00` are the 31st.
#[must_use]
pub fn parse_date_with_optional_offset(text: &str) -> Option<NaiveDate> {
    let (date, offset) = text.split_at_checked(10)?;
    if !offset.is_empty() {
        parse_offset(offset)?;
    }
    parse_date(date)
}

/// `Z` (either case) or `±HH:MM` / `±HH:MM:SS`, at most 18 hours, as seconds east of UTC. Java:
/// `DateTimeFormatterBuilder.appendOffsetId()` in a case-insensitive formatter.
#[must_use]
pub fn parse_offset(text: &str) -> Option<i32> {
    if text.eq_ignore_ascii_case("z") {
        return Some(0);
    }
    let (sign, rest) = match text.split_at_checked(1)? {
        ("+", rest) => (1, rest),
        ("-", rest) => (-1, rest),
        _ => return None,
    };
    let number = |part: &str| {
        (part.len() == 2 && part.bytes().all(|b| b.is_ascii_digit()))
            .then(|| part.parse::<i32>().ok())
            .flatten()
    };
    let parts: Vec<&str> = rest.split(':').collect();
    let (hours, minutes, seconds) = match parts.as_slice() {
        [hours, minutes] => (number(hours)?, number(minutes)?, 0),
        [hours, minutes, seconds] => (number(hours)?, number(minutes)?, number(seconds)?),
        _ => return None,
    };
    if hours > 18 || minutes > 59 || seconds > 59 || (hours == 18 && (minutes, seconds) != (0, 0)) {
        return None;
    }
    Some(sign * (hours * 3600 + minutes * 60 + seconds))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_are_strict() {
        assert_eq!(parse_date("2026-11-05"), NaiveDate::from_ymd_opt(2026, 11, 5));
        for bad in [
            "2026-11-5",
            "2026-02-30",
            "26-11-05",
            "2026/11/05",
            "yesterday",
            "+2026-11-05",
            "٢٠٢٦-11-05",
        ] {
            assert_eq!(parse_date(bad), None, "{bad}");
        }
    }

    #[test]
    fn an_offset_after_a_date_is_read_and_ignored() {
        let expected = NaiveDate::from_ymd_opt(2026, 1, 31);
        for text in [
            "2026-01-31",
            "2026-01-31Z",
            "2026-01-31z",
            "2026-01-31+01:00",
            "2026-01-31-18:00",
        ] {
            assert_eq!(parse_date_with_optional_offset(text), expected, "{text}");
        }
        for bad in ["2026-01-31 ", "2026-01-31+1", "2026-01-31+19:00", "2026-01-31T00:00"] {
            assert_eq!(parse_date_with_optional_offset(bad), None, "{bad}");
        }
    }

    #[test]
    fn offsets() {
        assert_eq!(parse_offset("Z"), Some(0));
        assert_eq!(parse_offset("+01:30"), Some(5400));
        assert_eq!(parse_offset("-00:00:30"), Some(-30));
        assert_eq!(parse_offset("+18:00"), Some(64800));
        for bad in ["", "+18:01", "+1:00", "01:00", "+01", "+01:60"] {
            assert_eq!(parse_offset(bad), None, "{bad}");
        }
    }
}
