//! Lesson 85: `Cow`, clone on write. Run `cargo test ex85`.

use std::borrow::Cow;

pub fn expand_tabs(text: &str) -> Cow<'_, str> {
    if text.contains('\t') { Cow::Owned(text.replace('\t', "  ")) } else { Cow::Borrowed(text) }
}

pub fn with_csv_suffix(name: &str) -> Cow<'_, str> {
    if name.ends_with(".csv") { Cow::Borrowed(name) } else { Cow::Owned(format!("{name}.csv")) }
}

pub fn expanded_len(text: &str) -> usize {
    expand_tabs(text).chars().count()
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
