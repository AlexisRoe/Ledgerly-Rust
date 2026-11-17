//! Lesson 85: `Cow`, clone on write. Run `cargo test ex85`.

use std::borrow::Cow;

/// 1. Replace every tab with two spaces. Most texts have no tab: then return the input borrowed
///    (`Cow::Borrowed(text)`) and allocate nothing. Only with a tab, return `Cow::Owned(…replace…)`.
pub fn expand_tabs(text: &str) -> Cow<'_, str> {
    todo!()
}

/// 2. A file name that ends in `.csv`: borrowed if it already does, owned with the suffix added otherwise.
pub fn with_csv_suffix(name: &str) -> Cow<'_, str> {
    todo!()
}

/// 3. Callers treat a `Cow<str>` like a `&str` (it derefs to one) and call `.into_owned()` only when they
///    must keep it. Return the length in chars of the expanded text, without converting it to a String.
pub fn expanded_len(text: &str) -> usize {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn borrows_when_nothing_changes() {
        assert!(matches!(expand_tabs("no tabs"), Cow::Borrowed("no tabs")));
        let expanded = expand_tabs("a\tb");
        assert!(matches!(expanded, Cow::Owned(_)));
        assert_eq!(expanded, "a  b");
    }

    #[test]
    fn adds_the_suffix_only_when_missing() {
        assert!(matches!(with_csv_suffix("march.csv"), Cow::Borrowed(_)));
        assert_eq!(with_csv_suffix("march"), "march.csv");
    }

    #[test]
    fn uses_a_cow_like_a_str() {
        assert_eq!(expanded_len("\t€"), 3);
    }
}
