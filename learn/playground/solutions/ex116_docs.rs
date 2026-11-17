//! Lesson 116: documentation as code. Run `cargo test ex116` (unit tests) and `cargo test --doc ex116` (the
//! examples in the docs below). `cargo doc --open` shows the rendered page.

/// Turns a title into a URL slug: lowercase, words joined by `-`, everything but ASCII letters and digits
/// dropped.
///
/// # Examples
///
/// ```
/// use playground::ex116_docs::slug;
///
/// assert_eq!(slug("Hello, World!"), "hello-world");
/// assert_eq!(slug("  Q3   report 2026 "), "q3-report-2026");
/// ```
pub fn slug(title: &str) -> String {
    title
        .split_whitespace()
        .map(|word| word.chars().filter(char::is_ascii_alphanumeric).collect::<String>().to_ascii_lowercase())
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

/// Splits `total` cents into `parts` shares that differ by at most one cent; the first shares get the extra cents.
///
/// # Errors
///
/// [`SplitError::NoParts`] if `parts` is zero.
///
/// # Examples
///
/// 1. This example is wrong on purpose: run `cargo test --doc ex116`, read the failure, and fix the
///    EXPECTED value (not the function).
///
/// ```
/// use playground::ex116_docs::split;
///
/// assert_eq!(split(100, 3), Ok(vec![34, 33, 33]));
/// ```
pub fn split(total: u64, parts: u64) -> Result<Vec<u64>, SplitError> {
    if parts == 0 {
        return Err(SplitError::NoParts);
    }
    let base = total / parts;
    let extra = total % parts;
    Ok((0..parts).map(|index| base + u64::from(index < extra)).collect())
}

/// Why [`split`] refused.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum SplitError {
    #[error("cannot split into zero parts")]
    NoParts,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugs() {
        assert_eq!(slug("Taxi & hotel"), "taxi-hotel");
    }

    #[test]
    fn splits() {
        assert_eq!(split(10, 0), Err(SplitError::NoParts));
        assert_eq!(split(10, 2), Ok(vec![5, 5]));
    }
}
