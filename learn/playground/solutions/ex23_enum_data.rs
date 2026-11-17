//! Lesson 23: enums with data. Run `cargo test ex23`.

/// What happened to an expense.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// No data.
    Created,
    /// Tuple-like: the id of the user who submitted.
    Submitted(u32),
    /// Struct-like: who rejected, and why.
    Rejected { by: u32, reason: String },
    Edited { changed_fields: Vec<String> },
}

/// 2. A one-line description for each event:
///   Created                         → "created"
///   Submitted(7)                    → "submitted by user 7"
///   Rejected { by: 3, reason: "no receipt" } → "rejected by user 3: no receipt"
///   Edited { changed_fields: ["merchant", "note"] } → "edited: merchant, note"
/// Use a `match` without a `_` arm. Hint for Edited: `changed_fields.join(", ")`.
pub fn describe(event: &Event) -> String {
    match event {
        Event::Created => "created".to_owned(),
        Event::Submitted(by) => format!("submitted by user {by}"),
        Event::Rejected { by, reason } => format!("rejected by user {by}: {reason}"),
        Event::Edited { changed_fields } => format!("edited: {}", changed_fields.join(", ")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn describes_every_kind() {
        assert_eq!(describe(&Event::Created), "created");
        assert_eq!(describe(&Event::Submitted(7)), "submitted by user 7");
        assert_eq!(
            describe(&Event::Rejected { by: 3, reason: "no receipt".to_owned() }),
            "rejected by user 3: no receipt"
        );
    }

    #[test]
    fn describes_edits() {
        let event = Event::Edited { changed_fields: vec!["merchant".to_owned(), "note".to_owned()] };
        assert_eq!(describe(&event), "edited: merchant, note");
    }
}
