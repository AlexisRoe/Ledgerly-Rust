//! Query parameters, read and converted the way Tomcat and Spring MVC do it for `@RequestParam`.
//!
//! Java counterpart: Tomcat's parameter parsing, Spring's `RequestParamMethodArgumentResolver` and the default
//! `ConversionService` (`StringToNumberConverterFactory`, `StringToEnumConverterFactory`,
//! `StringToUUIDConverter`, `ArrayToObjectConverter`, `ArrayToStringConverter`). Where conversion fails, Spring
//! answers with its own type-mismatch 400 (`REQUEST_REJECTED`), the same as for a path parameter.
//!
//! Spring's rules, as reproduced here:
//!
//! - `+` is a space, `%XX` escapes are UTF-8. A parameter without `=` has the value `""`.
//! - A parameter given more than once: a `String` parameter receives the values joined with `,`; any other type
//!   receives the first value.
//! - `""` for a parameter with a default value means the default.
//! - An empty value is no value for enums; a blank one is no value for UUIDs and dates.

use std::convert::Infallible;

use axum::extract::FromRequestParts;
use axum::http::StatusCode;
use axum::http::request::Parts;
use uuid::Uuid;

use super::problem::Problem;
use super::routing::percent_decode;
use crate::money::decimal_text::to_ascii_digit;
use crate::text::is_java_whitespace;

/// The decoded parameters of the request's query string, in the order they were sent.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct QueryParameters(Vec<(String, String)>);

impl QueryParameters {
    /// Splits and decodes a query string (the part after `?`).
    #[must_use]
    pub fn parse(query: &str) -> Self {
        let decode = |text: &str| percent_decode(&text.replace('+', " "));
        Self(
            query
                .split('&')
                .filter(|pair| !pair.is_empty())
                .map(|pair| pair.split_once('=').unwrap_or((pair, "")))
                .map(|(name, value)| (decode(name), decode(value)))
                .filter(|(name, _)| !name.is_empty())
                .collect(),
        )
    }

    fn values<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a str> + 'a {
        self.0
            .iter()
            .filter(move |(known, _)| known == name)
            .map(|(_, value)| value.as_str())
    }

    /// The first value, which is what Spring converts for a parameter that is not a `String`.
    #[must_use]
    pub fn first<'a>(&'a self, name: &'a str) -> Option<&'a str> {
        self.values(name).next()
    }

    /// A `String` parameter: the value, or all values joined with `,`. `None` if the parameter is absent.
    #[must_use]
    pub fn text(&self, name: &str) -> Option<String> {
        let values: Vec<&str> = self.values(name).collect();
        (!values.is_empty()).then(|| values.join(","))
    }

    /// A `String` parameter with a default value, which also stands in for `""`.
    #[must_use]
    pub fn text_or(&self, name: &str, default: &str) -> String {
        match self.text(name) {
            Some(value) if !value.is_empty() => value,
            _ => default.to_owned(),
        }
    }

    /// An `Integer` parameter with a default value, which also stands in for `""`.
    ///
    /// # Errors
    ///
    /// Spring's type-mismatch 400 if the value is not a Java `int`.
    pub fn integer_or(&self, name: &str, default: i32) -> Result<i32, Problem> {
        match self.first(name) {
            None | Some("") => Ok(default),
            Some(value) => java_int(value).ok_or_else(|| type_mismatch(name, value)),
        }
    }

    /// An enum or other parameter whose converter treats `""` as no value. `parse` gets the value as Spring's
    /// `StringToEnumConverterFactory` passes it on, trimmed with `String.trim()`.
    ///
    /// # Errors
    ///
    /// Spring's type-mismatch 400 if `parse` returns `None`.
    pub fn convert<T>(&self, name: &str, parse: impl FnOnce(&str) -> Option<T>) -> Result<Option<T>, Problem> {
        match self.first(name) {
            None | Some("") => Ok(None),
            Some(value) => parse(java_trim(value))
                .map(Some)
                .ok_or_else(|| type_mismatch(name, value)),
        }
    }

    /// A parameter whose converter treats a blank value as no value (`StringUtils.hasText`), as Spring's
    /// UUID and date converters do. `parse` gets the value as it was sent.
    ///
    /// # Errors
    ///
    /// Spring's type-mismatch 400 if `parse` returns `None`.
    pub fn convert_unless_blank<T>(
        &self,
        name: &str,
        parse: impl FnOnce(&str) -> Option<T>,
    ) -> Result<Option<T>, Problem> {
        match self.first(name) {
            Some(value) if !value.chars().all(is_java_whitespace) => {
                parse(value).map(Some).ok_or_else(|| type_mismatch(name, value))
            }
            _ => Ok(None),
        }
    }

    /// A `UUID` parameter. Spring trims the value with `String.trim()` before `UUID.fromString`.
    ///
    /// # Errors
    ///
    /// Spring's type-mismatch 400 if the value is not a UUID.
    pub fn uuid(&self, name: &str) -> Result<Option<Uuid>, Problem> {
        self.convert_unless_blank(name, |value| super::extract::parse_uuid(java_trim(value)))
    }
}

/// Handlers take the query as an extractor; a request without one has no parameters.
impl<S: Send + Sync> FromRequestParts<S> for QueryParameters {
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Infallible> {
        Ok(parts.uri.query().map(Self::parse).unwrap_or_default())
    }
}

/// Spring's 400 for a value its converter refused.
fn type_mismatch(name: &str, value: &str) -> Problem {
    Problem::rejected(
        StatusCode::BAD_REQUEST,
        format!("Failed to convert '{name}' with value: '{value}'"),
    )
}

/// `String.trim()`: without leading and trailing characters up to U+0020.
fn java_trim(text: &str) -> &str {
    text.trim_matches(|c: char| c <= ' ')
}

/// Spring's `NumberUtils.parseNumber(text, Integer.class)`: all whitespace removed, then `Integer.decode` for
/// `0x`, `0X` and `#` (hex) or `Integer.valueOf` otherwise, which takes a sign and Unicode decimal digits.
fn java_int(text: &str) -> Option<i32> {
    let trimmed: String = text.chars().filter(|c| !is_java_whitespace(*c)).collect();
    let (negative, unsigned) = match trimmed.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, trimmed.as_str()),
    };
    let hex = ["0x", "0X", "#"]
        .iter()
        .find_map(|prefix| unsigned.strip_prefix(prefix));
    let magnitude = match hex {
        Some(digits) => {
            if digits.starts_with(['-', '+']) {
                return None;
            }
            i64::from_str_radix(digits, 16).ok()?
        }
        None => {
            let digits = if negative {
                unsigned
            } else {
                unsigned.strip_prefix('+').unwrap_or(unsigned)
            };
            if digits.is_empty() || digits.len() > 40 {
                return None;
            }
            digits.chars().try_fold(0_i64, |value, c| {
                let digit = to_ascii_digit(c).to_digit(10)?;
                value.checked_mul(10)?.checked_add(i64::from(digit))
            })?
        }
    };
    i32::try_from(if negative { -magnitude } else { magnitude }).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plus_is_a_space_and_escapes_are_utf8() {
        let query = QueryParameters::parse("merchant=Caf%C3%A9+M%C3%BCller&plus=%2B&flag&=x&&empty=");
        assert_eq!(query.first("merchant"), Some("Café Müller"));
        assert_eq!(query.first("plus"), Some("+"));
        assert_eq!(query.first("flag"), Some(""));
        assert_eq!(query.first("empty"), Some(""));
        assert_eq!(query.first(""), None);
    }

    #[test]
    fn strings_join_repeated_values_and_others_take_the_first() {
        let query = QueryParameters::parse("sort=incurredOn&sort=asc&size=5&size=7");
        assert_eq!(query.text("sort").as_deref(), Some("incurredOn,asc"));
        assert_eq!(query.integer_or("size", 20).unwrap(), 5);
        assert_eq!(query.text("missing"), None);
    }

    #[test]
    fn an_empty_value_means_the_default() {
        let query = QueryParameters::parse("size=&sort=");
        assert_eq!(query.integer_or("size", 20).unwrap(), 20);
        assert_eq!(query.text_or("sort", "incurredOn,desc"), "incurredOn,desc");
    }

    #[test]
    fn integers_are_read_as_spring_reads_them() {
        for (text, expected) in [
            ("42", Some(42)),
            ("+42", Some(42)),
            ("-1", Some(-1)),
            (" 4 2 ", Some(42)),
            ("0x10", Some(16)),
            ("#10", Some(16)),
            ("-0X10", Some(-16)),
            ("-0x80000000", Some(i32::MIN)),
            ("2147483647", Some(i32::MAX)),
            ("2147483648", None),
            ("٤٢", Some(42)),
            ("abc", None),
            ("1.0", None),
            ("-", None),
            ("+", None),
            ("0x-1", None),
            ("1e3", None),
        ] {
            assert_eq!(java_int(text), expected, "{text}");
        }
    }

    #[test]
    fn a_value_that_does_not_convert_is_springs_type_mismatch() {
        let query = QueryParameters::parse("size=abc");
        let problem = query.integer_or("size", 20).unwrap_err();
        assert_eq!(problem.status(), StatusCode::BAD_REQUEST);
        assert_eq!(problem.detail(), Some("Failed to convert 'size' with value: 'abc'"));
    }

    #[test]
    fn enums_are_trimmed_and_empty_is_none() {
        let query = QueryParameters::parse("a=+DRAFT+&b=&c=+");
        let parse = |value: &str| (value == "DRAFT").then_some(());
        assert_eq!(query.convert("a", parse).unwrap(), Some(()));
        assert_eq!(query.convert("b", parse).unwrap(), None);
        assert!(query.convert("c", parse).is_err());
        assert_eq!(query.convert("absent", parse).unwrap(), None);
    }

    #[test]
    fn uuids_are_trimmed_and_blank_is_none() {
        let id = "0192f3a0-0000-7000-8000-000000000001";
        let query = QueryParameters::parse(&format!("a=+{id}+&b=+&c=nope"));
        assert_eq!(query.uuid("a").unwrap().map(|u| u.to_string()).as_deref(), Some(id));
        assert_eq!(query.uuid("b").unwrap(), None);
        assert!(query.uuid("c").is_err());
    }
}
