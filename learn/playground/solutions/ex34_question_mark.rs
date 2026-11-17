//! Lesson 34: early exit with `?`. Run `cargo test ex34`.

#[derive(Debug, PartialEq)]
pub enum ParseError {
    NotANumber,
    Negative,
}

pub fn parse_number(text: &str) -> Result<i64, ParseError> {
    text.trim().parse::<i64>().map_err(|_| ParseError::NotANumber)
}

pub fn non_negative(n: i64) -> Result<i64, ParseError> {
    if n < 0 { Err(ParseError::Negative) } else { Ok(n) }
}

/// Works, but it's a pyramid. Rewrite the body with two `?` and no `match`. The tests must stay green.
pub fn parse_quantity(text: &str) -> Result<i64, ParseError> {
    let n = parse_number(text)?;
    let q = non_negative(n)?;
    Ok(q)
}

/// `?` on Option: return the first character of the second word, or None if there's no second word or it's empty.
/// Hints: `text.split_whitespace().nth(1)?` and `.chars().next()`.
pub fn initial_of_second_word(text: &str) -> Option<char> {
    text.split_whitespace().nth(1)?.chars().next()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quantity() {
        assert_eq!(parse_quantity(" 12 "), Ok(12));
        assert_eq!(parse_quantity("x"), Err(ParseError::NotANumber));
        assert_eq!(parse_quantity("-3"), Err(ParseError::Negative));
    }

    #[test]
    fn second_initial() {
        assert_eq!(initial_of_second_word("Deutsche Bahn"), Some('B'));
        assert_eq!(initial_of_second_word("Bahn"), None);
    }
}
