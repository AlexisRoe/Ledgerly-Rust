//! Lesson 9: statements and expressions. Run `cargo test ex09`.
//!
//! Make the tests pass. Don't use the `return` keyword: let the last expression be the value.

/// Should return the price including 19 % VAT, in cents: 1000 → 1190.
pub fn with_vat(cents: u32) -> u32 {
    // Write the body as one expression, without `return` and without a trailing semicolon.
    // Then add a `;` at the end of your line, run `cargo test ex09` and read error E0308. Remove it again.
    todo!()
}

/// Should return the larger of the two numbers. Use a block or an `if` as an expression.
pub fn larger(a: i32, b: i32) -> i32 {
    todo!()
}

/// Should return the label "even" or "odd". (`&'static str` is fixed text; lesson 18 explains the type.)
pub fn parity(n: u32) -> &'static str {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vat_is_added() {
        assert_eq!(with_vat(1000), 1190);
        assert_eq!(with_vat(0), 0);
    }

    #[test]
    fn larger_picks_the_bigger_one() {
        assert_eq!(larger(3, 7), 7);
        assert_eq!(larger(-1, -5), -1);
    }

    #[test]
    fn parity_names_even_and_odd() {
        assert_eq!(parity(4), "even");
        assert_eq!(parity(7), "odd");
    }
}
