//! Lesson 9: statements and expressions. Run `cargo test ex09`.

/// Should return the price including 19 % VAT, in cents: 1000 → 1190.
pub fn with_vat(cents: u32) -> u32 {
    cents * 119 / 100
}

/// Should return the larger of the two numbers.
pub fn larger(a: i32, b: i32) -> i32 {
    if a > b { a } else { b }
}

/// Should return the label "even" or "odd".
pub fn parity(n: u32) -> &'static str {
    if n % 2 == 0 { "even" } else { "odd" }
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
