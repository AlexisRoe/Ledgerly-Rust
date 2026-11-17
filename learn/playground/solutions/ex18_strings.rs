//! Lesson 18: String vs &str. Run `cargo test ex18`.

/// Returns the text without leading and trailing spaces, as a view into the input (no copy).
/// Hint: `str` has a `trim` method that returns `&str`.
pub fn tidy(text: &str) -> &str {
    text.trim()
}

/// Builds a label like "TRAVEL: Deutsche Bahn". The result is new text, so it's a String.
/// Hint: `format!`.
pub fn label(category: &str, merchant: &str) -> String {
    format!("{category}: {merchant}")
}

/// Counts characters the way a person would ("€" is 1, "😀" is 1), not bytes.
/// Hint: `.chars()` gives an iterator; iterators have `.count()`.
pub fn characters(text: &str) -> usize {
    text.chars().count()
}

/// Counts the length the way JavaScript and Java do (UTF-16 units: "😀" is 2).
/// Hint: `.encode_utf16()`.
pub fn js_length(text: &str) -> usize {
    text.encode_utf16().count()
}

/// A merchant name that owns its text, like Ledgerly's `Merchant`.
pub struct Merchant(String);

impl Merchant {
    /// Stores the tidied text. You need to turn a `&str` into an owned `String`.
    pub fn new(text: &str) -> Merchant {
        Merchant(tidy(text).to_owned())
    }

    /// Lends the text out without copying.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tidy_trims() {
        assert_eq!(tidy("  Deutsche Bahn \n"), "Deutsche Bahn");
    }

    #[test]
    fn label_formats() {
        assert_eq!(label("TRAVEL", "Deutsche Bahn"), "TRAVEL: Deutsche Bahn");
    }

    #[test]
    fn lengths_differ() {
        assert_eq!("😀".len(), 4); // bytes: already true, just look at it
        assert_eq!(characters("😀€a"), 3);
        assert_eq!(js_length("😀€a"), 4);
    }

    #[test]
    fn merchant_owns_and_lends() {
        let input = String::from("  Café Kranzler ");
        let merchant = Merchant::new(&input);
        drop(input); // the merchant must not depend on the input any more
        assert_eq!(merchant.as_str(), "Café Kranzler");
    }
}
