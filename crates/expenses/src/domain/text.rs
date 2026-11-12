//! The free-text fields of an expense, as types that can only hold valid text.
//!
//! Java counterpart: the private `validMerchant` and `validNote` methods and the reason check in `reject` of
//! `expenses/internal/domain/Expense.java`. Java checks a `String` every time it enters the aggregate; here the
//! check runs once, in the constructor, and a `Merchant` that exists is valid.
//!
//! Stripping, blank checks and lengths follow Java's `String` (see `ledgerly_shared::text`).

use std::fmt;

use ledgerly_shared::text;

/// A single field value that breaks a rule. `field` is the name the API uses for it; the message is Java's.
///
/// Java counterpart: `expenses/internal/domain/InvalidInput.java`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct InvalidInput {
    pub field: &'static str,
    pub message: String,
}

impl InvalidInput {
    pub(crate) fn new(field: &'static str, message: impl Into<String>) -> Self {
        Self {
            field,
            message: message.into(),
        }
    }
}

/// Where the money was spent. Never blank, stripped, at most [`Merchant::MAX_LEN`] long.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Merchant(String);

impl Merchant {
    /// The longest merchant accepted, counted like Java's `String.length()`.
    pub const MAX_LEN: usize = 200;

    /// # Errors
    ///
    /// `InvalidInput` for field `merchant` if the text is blank, or longer than the cap after stripping.
    pub fn new(text: &str) -> Result<Self, InvalidInput> {
        if text::is_blank(text) {
            return Err(InvalidInput::new("merchant", "Merchant is required"));
        }
        let stripped = text::strip(text);
        if text::utf16_len(stripped) > Self::MAX_LEN {
            return Err(InvalidInput::new(
                "merchant",
                format!("Merchant is longer than {}", Self::MAX_LEN),
            ));
        }
        Ok(Self(stripped.to_owned()))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// An optional remark on an expense, kept exactly as written; at most [`Note::MAX_LEN`] long.
///
/// An expense without a note has `None`, not an empty `Note`. Java does not strip notes and accepts empty ones.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Note(String);

impl Note {
    /// The longest note accepted, counted like Java's `String.length()`.
    pub const MAX_LEN: usize = 2000;

    /// # Errors
    ///
    /// `InvalidInput` for field `note` if the text is longer than the cap.
    pub fn new(text: &str) -> Result<Self, InvalidInput> {
        if text::utf16_len(text) > Self::MAX_LEN {
            return Err(InvalidInput::new(
                "note",
                format!("Note is longer than {}", Self::MAX_LEN),
            ));
        }
        Ok(Self(text.to_owned()))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The explanation an approver gives when rejecting. Never blank, stripped, at most
/// [`RejectionReason::MAX_LEN`] long.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RejectionReason(String);

impl RejectionReason {
    /// The longest reason accepted, counted like Java's `String.length()`.
    pub const MAX_LEN: usize = 1000;

    /// # Errors
    ///
    /// `InvalidInput` for field `reason` if the text is blank, or longer than the cap after stripping.
    pub fn new(text: &str) -> Result<Self, InvalidInput> {
        if text::is_blank(text) {
            return Err(InvalidInput::new("reason", "A rejection reason is required"));
        }
        let stripped = text::strip(text);
        if text::utf16_len(stripped) > Self::MAX_LEN {
            return Err(InvalidInput::new(
                "reason",
                format!("Rejection reason is longer than {}", Self::MAX_LEN),
            ));
        }
        Ok(Self(stripped.to_owned()))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

macro_rules! display_as_text {
    ($($name:ident),+) => {
        $(impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        })+
    };
}

display_as_text!(Merchant, Note, RejectionReason);

#[cfg(test)]
mod tests {
    use super::*;

    fn invalid(field: &'static str, message: &str) -> InvalidInput {
        InvalidInput {
            field,
            message: message.to_owned(),
        }
    }

    // --- Merchant: the values of Java's ExpenseTest.Creation ------------------------------------------------

    #[test]
    fn merchant_is_required() {
        for blank in ["", "   ", "\t\n", "\u{3000}"] {
            assert_eq!(
                Merchant::new(blank),
                Err(invalid("merchant", "Merchant is required")),
                "{blank:?}"
            );
        }
    }

    #[test]
    fn merchant_is_stripped() {
        assert_eq!(Merchant::new("  Cafe Central \n").unwrap().as_str(), "Cafe Central");
        assert_eq!(Merchant::new("x").unwrap().as_str(), "x");
    }

    #[test]
    fn merchant_is_capped_at_200() {
        assert_eq!(Merchant::MAX_LEN, 200);
        assert_eq!(Merchant::new(&"m".repeat(200)).unwrap().as_str().len(), 200);
        assert_eq!(
            Merchant::new(&"m".repeat(201)),
            Err(invalid("merchant", "Merchant is longer than 200"))
        );
    }

    #[test]
    fn merchant_cap_applies_after_stripping() {
        let padded = format!("   {}   ", "m".repeat(200));
        assert_eq!(Merchant::new(&padded).unwrap().as_str(), "m".repeat(200));
    }

    #[test]
    fn merchant_length_is_counted_like_javas_string_length() {
        // 100 emoji are 100 characters but 200 UTF-16 units: the last length Java accepts.
        assert!(Merchant::new(&"😀".repeat(100)).is_ok());
        assert_eq!(
            Merchant::new(&format!("{}x", "😀".repeat(100))),
            Err(invalid("merchant", "Merchant is longer than 200"))
        );
    }

    #[test]
    fn a_merchant_of_only_non_breaking_spaces_is_accepted_as_in_java() {
        // Java's isBlank() does not count U+00A0 as whitespace; Rust's trim() would.
        assert_eq!(Merchant::new("\u{00A0}").unwrap().as_str(), "\u{00A0}");
    }

    // --- Note -------------------------------------------------------------------------------------------------

    #[test]
    fn note_is_capped_at_2000() {
        assert_eq!(Note::MAX_LEN, 2000);
        assert_eq!(Note::new(&"n".repeat(2000)).unwrap().as_str().len(), 2000);
        assert_eq!(
            Note::new(&"n".repeat(2001)),
            Err(invalid("note", "Note is longer than 2000"))
        );
    }

    #[test]
    fn note_is_kept_exactly_as_written() {
        // Unlike the merchant, Java neither strips a note nor rejects an empty or blank one.
        assert_eq!(Note::new("  Lunch \n").unwrap().as_str(), "  Lunch \n");
        assert_eq!(Note::new("").unwrap().as_str(), "");
        assert_eq!(Note::new("   ").unwrap().as_str(), "   ");
    }

    #[test]
    fn note_cap_counts_the_whitespace_too() {
        assert_eq!(
            Note::new(&format!(" {}", "n".repeat(2000))),
            Err(invalid("note", "Note is longer than 2000"))
        );
    }

    // --- RejectionReason: the values of Java's ExpenseTest.Transitions ---------------------------------------

    #[test]
    fn rejection_reason_is_required() {
        for blank in ["", "  ", "\t"] {
            assert_eq!(
                RejectionReason::new(blank),
                Err(invalid("reason", "A rejection reason is required")),
                "{blank:?}"
            );
        }
    }

    #[test]
    fn rejection_reason_is_stripped() {
        assert_eq!(
            RejectionReason::new("  no receipt for tip ").unwrap().as_str(),
            "no receipt for tip"
        );
    }

    #[test]
    fn rejection_reason_is_capped_at_1000() {
        assert_eq!(RejectionReason::MAX_LEN, 1000);
        assert_eq!(RejectionReason::new(&"r".repeat(1000)).unwrap().as_str().len(), 1000);
        assert_eq!(
            RejectionReason::new(&"r".repeat(1001)),
            Err(invalid("reason", "Rejection reason is longer than 1000"))
        );
        assert!(RejectionReason::new(&format!("  {}  ", "r".repeat(1000))).is_ok());
    }

    #[test]
    fn invalid_input_displays_its_message() {
        assert_eq!(Merchant::new("").unwrap_err().to_string(), "Merchant is required");
    }

    #[test]
    fn the_text_types_print_their_text() {
        assert_eq!(Merchant::new("Bahn").unwrap().to_string(), "Bahn");
        assert_eq!(Note::new("Ticket").unwrap().to_string(), "Ticket");
        assert_eq!(RejectionReason::new("no").unwrap().to_string(), "no");
    }
}
