//! Lesson 47: collecting into Result and Option. Run `cargo test ex47`.

#[derive(Debug, PartialEq)]
pub struct BadAmount(pub String);

fn parse(text: &str) -> Result<u32, BadAmount> {
    text.parse::<u32>().map_err(|_| BadAmount(text.to_owned()))
}

/// All or nothing: every amount parsed, or the first BadAmount. One iterator chain ending in `collect`.
pub fn parse_all(texts: &[&str]) -> Result<Vec<u32>, BadAmount> {
    todo!()
}

/// Keep going: return every BadAmount (in order), ignoring the good ones.
/// Hint: `.map(|t| parse(t))` then `.filter_map(|r| r.err())`.
pub fn all_errors(texts: &[&str]) -> Vec<BadAmount> {
    todo!()
}

/// An optional tip: None stays None, Some(text) must parse. Use `.map(parse).transpose()?`.
pub fn tip(text: Option<&str>) -> Result<Option<u32>, BadAmount> {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_or_nothing() {
        assert_eq!(parse_all(&["1", "2"]), Ok(vec![1, 2]));
        assert_eq!(parse_all(&["1", "x", "y"]), Err(BadAmount("x".to_owned())));
    }

    #[test]
    fn every_error() {
        assert_eq!(all_errors(&["1", "x", "3", "y"]), vec![BadAmount("x".to_owned()), BadAmount("y".to_owned())]);
    }

    #[test]
    fn optional() {
        assert_eq!(tip(None), Ok(None));
        assert_eq!(tip(Some("5")), Ok(Some(5)));
        assert_eq!(tip(Some("five")), Err(BadAmount("five".to_owned())));
    }
}
