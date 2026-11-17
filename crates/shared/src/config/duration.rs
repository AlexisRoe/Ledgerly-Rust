//! Durations as Spring Boot reads them from configuration (`DurationStyle`).

use std::time::Duration;

/// The text is not a duration this service accepts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("expected a duration such as 30s, 10m, 24h, 14d or PT1M")]
pub struct InvalidDuration;

/// Parses a duration in either of the two forms Spring Boot accepts.
///
/// - Simple: a whole number and a unit `ns`, `us`, `ms`, `s`, `m`, `h` or `d` (any case); without a unit the
///   number counts milliseconds.
/// - ISO 8601: `PT1M`, `PT1.5S`, `P7D`, `P1DT12H` (days, hours, minutes, seconds; any case).
///
/// # Errors
///
/// Returns [`InvalidDuration`] for anything else. Unlike Java, negative durations are refused: none of the
/// settings has a meaning for one.
pub fn parse_duration(text: &str) -> Result<Duration, InvalidDuration> {
    let text = text.trim();
    if text.starts_with(['P', 'p']) {
        return parse_iso(text);
    }
    let digits_end = text.find(|c: char| !c.is_ascii_digit()).unwrap_or(text.len());
    let (digits, unit) = text.split_at(digits_end);
    let number: u64 = digits.parse().map_err(|_| InvalidDuration)?;
    let nanos_per_unit: u64 = match unit.to_ascii_lowercase().as_str() {
        "ns" => 1,
        "us" => 1_000,
        "" | "ms" => 1_000_000,
        "s" => NANOS_PER_SECOND,
        "m" => 60 * NANOS_PER_SECOND,
        "h" => 3_600 * NANOS_PER_SECOND,
        "d" => 86_400 * NANOS_PER_SECOND,
        _ => return Err(InvalidDuration),
    };
    number
        .checked_mul(nanos_per_unit)
        .map(Duration::from_nanos)
        .ok_or(InvalidDuration)
}

const NANOS_PER_SECOND: u64 = 1_000_000_000;

/// `P[nD][T[nH][nM][n[.f]S]]`, with at least one part.
fn parse_iso(text: &str) -> Result<Duration, InvalidDuration> {
    let upper = text.to_ascii_uppercase();
    let body = upper.strip_prefix('P').ok_or(InvalidDuration)?;
    let (date, time) = match body.split_once('T') {
        Some((_, "")) => return Err(InvalidDuration),
        Some((date, time)) => (date, time),
        None => (body, ""),
    };
    let mut total = Duration::ZERO;
    let mut parts = 0_u32;

    let mut add = |amount: &str, seconds_per_unit: u64, fraction_allowed: bool| -> Result<(), InvalidDuration> {
        let (whole, fraction) = match amount.split_once('.') {
            Some((whole, fraction)) if fraction_allowed => (whole, fraction),
            Some(_) => return Err(InvalidDuration),
            None => (amount, ""),
        };
        if whole.is_empty() || !whole.bytes().all(|b| b.is_ascii_digit()) {
            return Err(InvalidDuration);
        }
        if fraction.len() > 9 || !fraction.bytes().all(|b| b.is_ascii_digit()) {
            return Err(InvalidDuration);
        }
        let seconds = whole
            .parse::<u64>()
            .ok()
            .and_then(|n| n.checked_mul(seconds_per_unit))
            .ok_or(InvalidDuration)?;
        let nanos: u32 = if fraction.is_empty() {
            0
        } else {
            format!("{fraction:0<9}").parse().map_err(|_| InvalidDuration)?
        };
        total = total
            .checked_add(Duration::new(seconds, nanos))
            .ok_or(InvalidDuration)?;
        parts = parts.saturating_add(1);
        Ok(())
    };

    let mut rest = date;
    if let Some((days, after)) = rest.split_once('D') {
        add(days, 86_400, false)?;
        rest = after;
    }
    if !rest.is_empty() {
        return Err(InvalidDuration);
    }
    let mut rest = time;
    for (designator, seconds_per_unit, fraction_allowed) in [('H', 3_600, false), ('M', 60, false), ('S', 1, true)] {
        if let Some((amount, after)) = rest.split_once(designator) {
            add(amount, seconds_per_unit, fraction_allowed)?;
            rest = after;
        }
    }
    if !rest.is_empty() || parts == 0 {
        return Err(InvalidDuration);
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_form_with_every_unit() {
        assert_eq!(parse_duration("10m"), Ok(Duration::from_secs(600)));
        assert_eq!(parse_duration("14d"), Ok(Duration::from_secs(14 * 86_400)));
        assert_eq!(parse_duration("24h"), Ok(Duration::from_secs(86_400)));
        assert_eq!(parse_duration("2s"), Ok(Duration::from_secs(2)));
        assert_eq!(parse_duration("250ms"), Ok(Duration::from_millis(250)));
        assert_eq!(parse_duration("5us"), Ok(Duration::from_micros(5)));
        assert_eq!(parse_duration("7ns"), Ok(Duration::from_nanos(7)));
        assert_eq!(parse_duration("30S"), Ok(Duration::from_secs(30)));
    }

    #[test]
    fn a_bare_number_counts_milliseconds_as_in_spring() {
        assert_eq!(parse_duration("1500"), Ok(Duration::from_millis(1500)));
    }

    #[test]
    fn iso_8601_form() {
        assert_eq!(parse_duration("PT1M"), Ok(Duration::from_secs(60)));
        assert_eq!(parse_duration("pt30s"), Ok(Duration::from_secs(30)));
        assert_eq!(parse_duration("P7D"), Ok(Duration::from_secs(7 * 86_400)));
        assert_eq!(parse_duration("P1DT12H"), Ok(Duration::from_secs(36 * 3_600)));
        assert_eq!(parse_duration("PT1H2M3S"), Ok(Duration::from_secs(3_723)));
        assert_eq!(parse_duration("PT1.5S"), Ok(Duration::from_millis(1500)));
    }

    #[test]
    fn everything_else_is_refused() {
        for text in [
            "",
            "m",
            "10 m",
            "10w",
            "1.5s",
            "-5s",
            "+5s",
            "P",
            "PT",
            "P1D T1H",
            "PT1S1M",
            "PT1.5M",
            "P1M",
            "P1Y",
            "PTS",
            "99999999999999999999s",
            "18446744073709551615d",
        ] {
            assert_eq!(parse_duration(text), Err(InvalidDuration), "{text:?}");
        }
    }
}
