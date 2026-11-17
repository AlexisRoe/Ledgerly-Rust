//! Lesson 25: Result. Run `cargo test ex25`.

#[derive(Debug, PartialEq)]
pub enum InvalidInput {
    Blank,
    TooLong { max: usize },
    NotPositive,
}

/// A merchant name: never blank, at most 20 characters (count with `.chars().count()`).
#[derive(Debug, PartialEq)]
pub struct Merchant(String);

impl Merchant {
    /// Validates and stores the trimmed text. Check blank first, then length.
    pub fn new(text: &str) -> Result<Self, InvalidInput> {
        todo!()
    }
}

/// An amount in cents that must be above zero.
#[derive(Debug, PartialEq)]
pub struct Cents(i64);

impl Cents {
    pub fn new(value: i64) -> Result<Self, InvalidInput> {
        todo!()
    }
}

/// Builds both, failing with the first error. Use `?` twice, then `Ok((merchant, cents))`.
pub fn line_item(merchant: &str, cents: i64) -> Result<(Merchant, Cents), InvalidInput> {
    todo!()
}

/// An HTTP-ish status for a line item: 201 if valid, 400 otherwise. Use `match` on `line_item(...)`.
pub fn status(merchant: &str, cents: i64) -> u16 {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merchant_rules() {
        assert_eq!(Merchant::new("  Bahn "), Ok(Merchant("Bahn".to_owned())));
        assert_eq!(Merchant::new("   "), Err(InvalidInput::Blank));
        assert_eq!(Merchant::new(&"x".repeat(21)), Err(InvalidInput::TooLong { max: 20 }));
    }

    #[test]
    fn cents_rules() {
        assert_eq!(Cents::new(1), Ok(Cents(1)));
        assert_eq!(Cents::new(0), Err(InvalidInput::NotPositive));
    }

    #[test]
    fn first_error_wins() {
        assert_eq!(line_item("", 0), Err(InvalidInput::Blank));
        assert_eq!(line_item("Bahn", 0), Err(InvalidInput::NotPositive));
        assert!(line_item("Bahn", 990).is_ok());
    }

    #[test]
    fn statuses() {
        assert_eq!(status("Bahn", 990), 201);
        assert_eq!(status("", 990), 400);
    }
}
