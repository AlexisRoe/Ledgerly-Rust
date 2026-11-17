//! Lesson 111: error design, libraries vs applications. Run `cargo test ex111`.

use std::num::ParseIntError;

use anyhow::Context as _;

/// LIBRARY style: a precise enum callers can `match` on. Each variant is a case the caller may handle
/// differently; `#[from]` lets `?` convert the underlying error.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum LimitError {
    #[error("limit is not a number")]
    NotANumber(#[from] ParseIntError),
    #[error("limit {0} is above the maximum of {max}", max = MAX_LIMIT)]
    TooLarge(u32),
}

pub const MAX_LIMIT: u32 = 100;

/// 1. Parse a page-size limit: `text.trim().parse::<u32>()?` (the `?` converts via `#[from]`), then reject
///    values above MAX_LIMIT with `LimitError::TooLarge(value)`.
pub fn parse_limit(text: &str) -> Result<u32, LimitError> {
    let value: u32 = text.trim().parse()?;
    if value > MAX_LIMIT {
        return Err(LimitError::TooLarge(value));
    }
    Ok(value)
}

/// A fake environment for the application-level function below.
pub fn read_setting(name: &str) -> Option<&'static str> {
    match name {
        "PAGE_LIMIT" => Some("250"),
        "TIMEOUT_SECS" => Some("30"),
        _ => None,
    }
}

/// APPLICATION style: nobody matches on these errors; they're reported. `anyhow::Error` holds any error,
/// and `.context(…)` / `.with_context(|| …)` add a line of explanation on the way up.
/// 2. Read `name` with `read_setting` (missing → `anyhow::anyhow!("{name} is not set")`, or
///    `.with_context(|| format!("{name} is not set"))` on the Option), then `parse_limit` it, adding the context
///    `format!("invalid {name}")` to a parse failure.
pub fn limit_setting(name: &str) -> anyhow::Result<u32> {
    let text = read_setting(name).with_context(|| format!("{name} is not set"))?;
    let limit = parse_limit(text).with_context(|| format!("invalid {name}"))?;
    Ok(limit)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn library_errors_can_be_matched() {
        assert_eq!(parse_limit(" 20 "), Ok(20));
        assert_eq!(parse_limit("500"), Err(LimitError::TooLarge(500)));
        assert!(matches!(parse_limit("lots"), Err(LimitError::NotANumber(_))));
        assert_eq!(LimitError::TooLarge(500).to_string(), "limit 500 is above the maximum of 100");
    }

    #[test]
    fn application_errors_carry_context() {
        assert_eq!(limit_setting("TIMEOUT_SECS").ok(), Some(30));
        let error = limit_setting("PAGE_LIMIT").err().map(|e| format!("{e:#}"));
        // `{:#}` prints the whole chain: context, then cause.
        assert_eq!(error.as_deref(), Some("invalid PAGE_LIMIT: limit 250 is above the maximum of 100"));
        let missing = limit_setting("NOPE").err().map(|e| e.to_string());
        assert_eq!(missing.as_deref(), Some("NOPE is not set"));
    }
}
