//! Lesson 56: thiserror. Run `cargo test ex56`.

/// A low-level error from parsing. Already done.
#[derive(Debug, PartialEq, thiserror::Error)]
#[error("not a number: {0}")]
pub struct NotANumber(pub String);

fn parse_cents(text: &str) -> Result<u32, NotANumber> {
    text.parse().map_err(|_| NotANumber(text.to_owned()))
}

// 1. Define `pub enum RefundError` deriving Debug, PartialEq, thiserror::Error, with:
//      - `Parse(#[from] NotANumber)`     message: transparent (the inner message)
//      - `TooHigh { requested: u32, max: u32 }` message: "refund of {requested} exceeds {max}"
//      - `Zero`                          message: "refund must not be zero"
// 2. Then uncomment `refund` and the tests. Notice: `parse_cents(text)?` converts NotANumber automatically.

// pub fn refund(text: &str, max: u32) -> Result<u32, RefundError> {
//     let cents = parse_cents(text)?;
//     if cents == 0 {
//         return Err(RefundError::Zero);
//     }
//     if cents > max {
//         return Err(RefundError::TooHigh { requested: cents, max });
//     }
//     Ok(cents)
// }

#[cfg(test)]
mod tests {
    use super::*;

    // #[test]
    // fn refunds() {
    //     assert_eq!(refund("500", 1000), Ok(500));
    //     assert_eq!(refund("0", 1000), Err(RefundError::Zero));
    //     assert_eq!(refund("abc", 1000), Err(RefundError::Parse(NotANumber("abc".to_owned()))));
    // }
    //
    // #[test]
    // fn messages() {
    //     assert_eq!(refund("2000", 1000).unwrap_err().to_string(), "refund of 2000 exceeds 1000");
    //     assert_eq!(refund("abc", 1000).unwrap_err().to_string(), "not a number: abc");
    //     assert_eq!(RefundError::Zero.to_string(), "refund must not be zero");
    // }
}
