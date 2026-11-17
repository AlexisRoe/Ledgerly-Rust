//! Lesson 91: the orphan rule and the newtype workaround. Run `cargo test ex91`.

use std::fmt;

pub struct Line {
    pub merchant: String,
    pub cents: u64,
}

// `impl fmt::Display for Vec<Line>` is rejected (E0117): both `Display` and `Vec` belong to std. Wrap it.

/// 1. A newtype around the foreign type. Implement `Display` for it: one line per item, "merchant: cents",
///    separated by "\n" (no trailing newline). Hint: `writeln!`/`write!` into `f`, or join a Vec<String>.
pub struct Report(pub Vec<Line>);

impl fmt::Display for Report {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        todo!()
    }
}

/// 2. The wrapper is also the place for methods that only make sense for this list.
impl Report {
    pub fn total(&self) -> u64 {
        todo!()
    }
}

/// 3. The other direction is allowed: YOUR trait on a foreign type. Implement it for `Vec<Line>`.
pub trait Biggest {
    fn biggest(&self) -> Option<&str>;
}

impl Biggest for Vec<Line> {
    /// The merchant of the line with the most cents.
    fn biggest(&self) -> Option<&str> {
        todo!()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines() -> Vec<Line> {
        vec![
            Line { merchant: "Taxi".to_owned(), cents: 1_250 },
            Line { merchant: "Hotel".to_owned(), cents: 12_000 },
        ]
    }

    #[test]
    fn displays_through_the_newtype() {
        let report = Report(lines());
        assert_eq!(report.to_string(), "Taxi: 1250\nHotel: 12000");
        assert_eq!(report.total(), 13_250);
    }

    #[test]
    fn local_trait_on_a_foreign_type() {
        assert_eq!(lines().biggest(), Some("Hotel"));
        assert_eq!(Vec::<Line>::new().biggest(), None);
    }
}
