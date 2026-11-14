//! Text handling that has to agree with Java's `String`, character for character.
//!
//! Ledgerly-Java trims with `String.strip()`, tests for blanks with `isBlank()` and measures with `length()`.
//! Rust's nearest methods answer differently at the edges, and those edges decide whether a request is accepted:
//!
//! | | Java | Rust |
//! | --- | --- | --- |
//! | non-breaking spaces U+00A0, U+2007, U+202F and U+0085 | not whitespace | whitespace for `trim()` |
//! | separators U+001C–U+001F | whitespace | not whitespace for `trim()` |
//! | length of `"😀"` | 2 (UTF-16 units) | 4 (`len()`, bytes) or 1 (`chars().count()`) |

/// `Character.isWhitespace`: Unicode space, line and paragraph separators except the non-breaking spaces, plus
/// tab, the line breaks and the four ASCII separator control characters.
#[must_use]
pub const fn is_java_whitespace(c: char) -> bool {
    matches!(
        c,
        '\u{0009}'..='\u{000D}'
            | '\u{001C}'..='\u{001F}'
            | '\u{0020}'
            | '\u{1680}'
            | '\u{2000}'..='\u{2006}'
            | '\u{2008}'..='\u{200A}'
            | '\u{2028}'
            | '\u{2029}'
            | '\u{205F}'
            | '\u{3000}'
    )
}

/// `String.strip()`: the text without leading and trailing Java whitespace.
#[must_use]
pub fn strip(text: &str) -> &str {
    text.trim_matches(is_java_whitespace)
}

/// `String.isBlank()`: empty, or nothing but Java whitespace.
#[must_use]
pub fn is_blank(text: &str) -> bool {
    text.chars().all(is_java_whitespace)
}

/// `String.length()`: the number of UTF-16 code units, which is what Java's length limits count.
#[must_use]
pub fn utf16_len(text: &str) -> usize {
    text.encode_utf16().count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_what_javas_strip_strips() {
        assert_eq!(strip("  Cafe Central \t\r\n"), "Cafe Central");
        assert_eq!(strip("\u{2003}wide\u{3000}"), "wide");
        assert_eq!(strip("\u{001C}separators\u{001F}"), "separators");
        assert_eq!(strip("inner  space"), "inner  space");
        assert_eq!(strip("   "), "");
    }

    #[test]
    fn keeps_what_javas_strip_keeps_although_rusts_trim_removes_it() {
        // Non-breaking spaces and NEXT LINE are not whitespace to Java's `Character.isWhitespace`.
        for kept in ["\u{00A0}", "\u{2007}", "\u{202F}", "\u{0085}"] {
            let text = format!("{kept}x{kept}");
            assert_eq!(strip(&text), text);
            assert_eq!(text.trim(), "x", "the premise: Rust's trim differs here");
            assert!(!is_blank(kept));
        }
    }

    #[test]
    fn blank_means_empty_or_only_java_whitespace() {
        assert!(is_blank(""));
        assert!(is_blank("   "));
        assert!(is_blank(
            " \t\n\u{000B}\u{000C}\r\u{001C}\u{001D}\u{001E}\u{001F}\u{1680}\u{2028}\u{2029}\u{205F}"
        ));
        assert!(!is_blank(" x "));
        assert!(!is_blank("\u{200B}"), "zero width space is not whitespace");
    }

    #[test]
    fn length_counts_utf16_units_like_javas_string_length() {
        assert_eq!(utf16_len(""), 0);
        assert_eq!(utf16_len("abc"), 3);
        assert_eq!(utf16_len("é"), 1);
        assert_eq!(utf16_len("😀"), 2);
        assert_eq!("😀".len(), 4, "the premise: bytes are something else");
        assert_eq!("😀".chars().count(), 1, "and so are characters");
    }
}
