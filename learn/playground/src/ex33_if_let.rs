//! Lesson 33: if let, let…else, while let, matches!. Run `cargo test ex33`.

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Status {
    Draft,
    Submitted,
    Approved,
    Reimbursed,
}

/// `if let`: return "Hello, <name>!" if a name is given, otherwise "Hello!".
pub fn greet(name: Option<&str>) -> String {
    todo!()
}

/// `let … else`: parse "<amount> <currency>" like "12 EUR". If there's no space, return Err("missing currency").
/// Hint: `text.split_once(' ')` returns `Option<(&str, &str)>`.
pub fn split_amount(text: &str) -> Result<(&str, &str), &'static str> {
    todo!()
}

/// `while let`: pop items off the stack and sum them until it's empty.
pub fn drain_sum(mut stack: Vec<u32>) -> u32 {
    let mut total = 0;
    todo!();
    total
}

/// `matches!`: approved and reimbursed expenses are final.
pub fn is_final(status: Status) -> bool {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn greets() {
        assert_eq!(greet(Some("Sam")), "Hello, Sam!");
        assert_eq!(greet(None), "Hello!");
    }

    #[test]
    fn splits() {
        assert_eq!(split_amount("12 EUR"), Ok(("12", "EUR")));
        assert_eq!(split_amount("12"), Err("missing currency"));
    }

    #[test]
    fn drains() {
        assert_eq!(drain_sum(vec![1, 2, 3]), 6);
    }

    #[test]
    fn finality() {
        assert!(is_final(Status::Approved));
        assert!(is_final(Status::Reimbursed));
        assert!(!is_final(Status::Draft));
        assert!(!is_final(Status::Submitted));
    }
}
