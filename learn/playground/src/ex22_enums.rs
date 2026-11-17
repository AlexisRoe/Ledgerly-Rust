//! Lesson 22: plain enums, Ledgerly style. Run `cargo test ex22`.

/// Where an expense stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Draft,
    Submitted,
    // 1. Add the variant `Approved` here, then fix what stops compiling.
}

impl Status {
    /// 2. Every status, in declaration order. Update it when you add `Approved`.
    pub const ALL: [Self; 2] = [Self::Draft, Self::Submitted];

    /// The wire name: "DRAFT", "SUBMITTED", "APPROVED".
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Draft => "DRAFT",
            Self::Submitted => "SUBMITTED",
        }
    }

    /// 3. The reverse: find the status whose `as_str` equals `text`, or None.
    /// Hint: loop over `Self::ALL` and `return Some(status)` when it matches; `None` after the loop.
    pub fn parse(text: &str) -> Option<Self> {
        todo!()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_round_trip() {
        for status in Status::ALL {
            assert_eq!(Status::parse(status.as_str()), Some(status));
        }
    }

    #[test]
    fn unknown_text_is_none() {
        assert_eq!(Status::parse("draft"), None);
        assert_eq!(Status::parse("PAID"), None);
    }

    #[test]
    fn there_are_three() {
        assert_eq!(Status::ALL.len(), 3);
        assert_eq!(Status::ALL.last().map(|s| s.as_str()), Some("APPROVED"));
    }
}
