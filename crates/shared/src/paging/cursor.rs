//! Java counterpart: `shared/paging/Cursor.java`.

use base64::Engine;
use base64::alphabet::URL_SAFE;
use base64::engine::DecodePaddingMode;
use base64::engine::general_purpose::{GeneralPurpose, GeneralPurposeConfig};
use chrono::{DateTime, NaiveDateTime, NaiveTime, SecondsFormat, TimeDelta, Timelike, Utc};
use serde::Serialize;
use uuid::Uuid;

use crate::iso;
use crate::web::Problem;

/// Java's `Base64.getUrlEncoder().withoutPadding()` for encoding and `Base64.getUrlDecoder()` for decoding,
/// which takes the token with or without its padding and does not look at the unused bits of the last
/// character.
const BASE64URL: GeneralPurpose = GeneralPurpose::new(
    &URL_SAFE,
    GeneralPurposeConfig::new()
        .with_encode_padding(false)
        .with_decode_padding_mode(DecodePaddingMode::Indifferent)
        .with_decode_allow_trailing_bits(true),
);

/// An opaque position in a keyset-paged list: the sort timestamp and the id of the last row seen.
///
/// Handed to clients as unpadded Base64url of `{"t": "<instant>", "id": "<uuid>"}`, so that they treat it as a
/// token and never build one themselves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cursor {
    pub timestamp: DateTime<Utc>,
    pub id: Uuid,
}

/// The token is not a cursor. Java: `InvalidRequest("cursor", "Invalid cursor")`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("Invalid cursor")]
pub struct InvalidCursor;

impl From<InvalidCursor> for Problem {
    /// 400 `VALIDATION_FAILED` for the field `cursor`.
    fn from(error: InvalidCursor) -> Self {
        Self::invalid_request("cursor", error.to_string())
    }
}

/// The JSON inside the token. Java writes a `Map.of`, whose key order varies from one JVM start to the next,
/// so Rust picks one and [`Cursor::decode`] accepts both.
#[derive(Serialize)]
struct Json {
    t: String,
    id: String,
}

impl Cursor {
    #[must_use]
    pub const fn new(timestamp: DateTime<Utc>, id: Uuid) -> Self {
        Self { timestamp, id }
    }

    /// The text handed to clients as `nextCursor`. Only `A`–`Z`, `a`–`z`, `0`–`9`, `-` and `_`.
    #[must_use]
    pub fn encode(&self) -> String {
        let json = Json {
            t: instant_to_string(self.timestamp),
            id: self.id.to_string(),
        };
        // Serialising two strings cannot fail; the fallback is never used.
        let text = serde_json::to_string(&json).unwrap_or_default();
        BASE64URL.encode(text)
    }

    /// Reads a token [`Cursor::encode`] (or Java) produced.
    ///
    /// # Errors
    ///
    /// [`InvalidCursor`] for anything else: bad Base64, bad JSON, missing members, a bad timestamp or UUID. All
    /// the same to the caller, as in Java.
    pub fn decode(token: &str) -> Result<Self, InvalidCursor> {
        let bytes = BASE64URL.decode(token).map_err(|_| InvalidCursor)?;
        // Java decodes the bytes with `new String(bytes, UTF_8)`, which replaces what is not UTF-8.
        let text = String::from_utf8_lossy(&bytes);
        let json: serde_json::Value = serde_json::from_str(&text).map_err(|_| InvalidCursor)?;
        let member = |name: &str| json.get(name).and_then(serde_json::Value::as_str).ok_or(InvalidCursor);
        let timestamp = parse_instant(member("t")?).ok_or(InvalidCursor)?;
        let id = crate::web::parse_uuid(member("id")?).ok_or(InvalidCursor)?;
        Ok(Self { timestamp, id })
    }
}

/// `Instant.toString()`: UTC with `Z`, and the fraction of a second in groups of three digits, as many as it
/// needs (none, milli, micro or nano).
fn instant_to_string(instant: DateTime<Utc>) -> String {
    instant.to_rfc3339_opts(SecondsFormat::AutoSi, true)
}

/// `Instant.parse` (`DateTimeFormatter.ISO_INSTANT`, case-insensitive): `yyyy-MM-ddTHH:mm:ss`, an optional
/// fraction of up to nine digits, then `Z` or an offset `±HH:MM[:SS]`, which is converted to UTC.
fn parse_instant(text: &str) -> Option<DateTime<Utc>> {
    let (date, rest) = text.split_at_checked(10)?;
    let rest = rest.strip_prefix(['T', 't'])?;
    let (time, rest) = rest.split_at_checked(8)?;
    let date = iso::parse_date(date)?;
    let time = parse_fixed_time(time)?;
    let (nanos, offset) = match rest.strip_prefix('.') {
        Some(fraction) => {
            let digits = fraction.bytes().take_while(u8::is_ascii_digit).count();
            if digits > 9 {
                return None;
            }
            let (digits, offset) = fraction.split_at_checked(digits)?;
            let nanos = format!("{digits:0<9}").parse::<u32>().ok()?;
            (nanos, offset)
        }
        None => (0, rest),
    };
    let seconds = iso::parse_offset(offset)?;
    let local = NaiveDateTime::new(date, time.with_nanosecond(nanos)?);
    let utc = local.checked_sub_signed(TimeDelta::seconds(i64::from(seconds)))?;
    Some(utc.and_utc())
}

/// `HH:mm:ss` with ASCII digits.
fn parse_fixed_time(text: &str) -> Option<NaiveTime> {
    let shaped = text.len() == 8
        && text.bytes().enumerate().all(|(index, byte)| {
            if matches!(index, 2 | 5) {
                byte == b':'
            } else {
                byte.is_ascii_digit()
            }
        });
    if !shaped {
        return None;
    }
    NaiveTime::from_hms_opt(
        text.get(0..2)?.parse().ok()?,
        text.get(3..5)?.parse().ok()?,
        text.get(6..8)?.parse().ok()?,
    )
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    fn id() -> Uuid {
        Uuid::parse_str("0192f3a0-0000-7000-8000-00000000002a").unwrap()
    }

    fn base64url(text: &str) -> String {
        BASE64URL.encode(text)
    }

    /// Java: `CursorTest.roundTrips`.
    #[test]
    fn round_trips() {
        let timestamp = Utc.with_ymd_and_hms(2026, 11, 5, 10, 15, 30).unwrap() + TimeDelta::microseconds(123_456);
        let cursor = Cursor::new(timestamp, Uuid::new_v4());
        assert_eq!(Cursor::decode(&cursor.encode()), Ok(cursor));
    }

    /// Java: `CursorTest.tokenIsUrlSafe`.
    #[test]
    fn the_token_is_url_safe() {
        let token = Cursor::new(Utc::now(), Uuid::new_v4()).encode();
        assert!(!token.is_empty());
        assert!(
            token
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'),
            "{token}"
        );
    }

    /// Port spec §3.6: the timestamp is `Instant.toString()`, the JSON keys are `t` and `id`.
    #[test]
    fn the_token_is_the_json_java_writes() {
        let at = |micros| Utc.with_ymd_and_hms(2026, 11, 12, 10, 15, 30).unwrap() + TimeDelta::microseconds(micros);
        for (micros, instant) in [
            (0, "2026-11-12T10:15:30Z"),
            (120_000, "2026-11-12T10:15:30.120Z"),
            (123_456, "2026-11-12T10:15:30.123456Z"),
            (100, "2026-11-12T10:15:30.000100Z"),
        ] {
            let token = Cursor::new(at(micros), id()).encode();
            let json = String::from_utf8(BASE64URL.decode(token).unwrap()).unwrap();
            assert_eq!(json, format!(r#"{{"t":"{instant}","id":"{}"}}"#, id()));
        }
    }

    /// A token as Java writes it, in either of the key orders its `Map.of` can produce, with or without
    /// padding.
    #[test]
    fn a_cursor_java_produced_decodes() {
        let expected = Cursor::new(
            Utc.with_ymd_and_hms(2026, 11, 12, 10, 15, 30).unwrap() + TimeDelta::microseconds(123_456),
            id(),
        );
        let id = id();
        for json in [
            format!(r#"{{"t":"2026-11-12T10:15:30.123456Z","id":"{id}"}}"#),
            format!(r#"{{"id":"{id}","t":"2026-11-12T10:15:30.123456Z"}}"#),
        ] {
            let token = base64url(&json);
            assert_eq!(Cursor::decode(&token), Ok(expected), "{json}");
            let padded = base64::engine::general_purpose::URL_SAFE.encode(&json);
            assert_eq!(Cursor::decode(&padded), Ok(expected), "{padded}");
        }
    }

    /// `Instant.parse` takes an offset and converts it to UTC; and `ISO_INSTANT` ignores case.
    #[test]
    fn offsets_and_lower_case_are_read_as_java_reads_them() {
        let expected = Utc.with_ymd_and_hms(2026, 11, 12, 9, 15, 30).unwrap();
        for instant in [
            "2026-11-12T10:15:30+01:00",
            "2026-11-12t09:15:30z",
            "2026-11-12T09:15:30.000Z",
        ] {
            assert_eq!(parse_instant(instant), Some(expected), "{instant}");
        }
    }

    /// Java: `CursorTest.junkIsA400` and `validBase64OfWrongJsonIsA400`.
    #[test]
    fn junk_is_an_invalid_cursor() {
        let mut junk: Vec<String> = ["", "!!!", "not-base64-%%", "e30", "bm90IGpzb24", "e30=="]
            .iter()
            .map(|token| (*token).to_owned())
            .collect();
        junk.push(base64::engine::general_purpose::URL_SAFE.encode(r#"{"t":"yesterday","id":"nope"}"#));
        for json in [
            r#"{"t":"2026-11-12T10:15:30Z"}"#.to_owned(),
            format!(r#"{{"t":"2026-11-12 10:15:30Z","id":"{}"}}"#, id()),
            format!(r#"{{"t":"2026-11-12T10:15:30","id":"{}"}}"#, id()),
            format!(r#"{{"t":"2026-11-12T10:15:30.1234567890Z","id":"{}"}}"#, id()),
            format!(r#"{{"t":12,"id":"{}"}}"#, id()),
            format!(r#"[{{"t":"2026-11-12T10:15:30Z","id":"{}"}}]"#, id()),
        ] {
            junk.push(base64url(&json));
        }
        for token in junk {
            assert_eq!(Cursor::decode(&token), Err(InvalidCursor), "{token}");
        }
    }

    #[test]
    fn an_invalid_cursor_is_a_validation_problem_for_the_field_cursor() {
        let problem = Problem::from(InvalidCursor);
        assert_eq!(problem.status(), axum::http::StatusCode::BAD_REQUEST);
        let violations = problem.violations();
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].field, "cursor");
        assert_eq!(violations[0].message, "Invalid cursor");
    }
}
