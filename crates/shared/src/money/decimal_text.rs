//! A decimal number the way Java's `BigDecimal` reads and prints it.
//!
//! Ledgerly-Java parses amounts with `new BigDecimal(String)` and puts `BigDecimal.toString()` into the messages
//! clients see. `rust_decimal` differs from both: it accepts `1_000`, rejects `1E+2`, cannot hold a negative scale
//! or more than 28 digits, and never prints an exponent. This module reproduces Java's grammar and Java's
//! `toString()`, so the same input is accepted and the same message comes back, whatever its size.

use std::fmt;

use rust_decimal::Decimal;

use super::unicode_digits::ZEROS;

/// `unscaled value × 10^-scale`, like a `BigDecimal`: any number of digits, and a scale that may be negative.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct JavaDecimal {
    /// Never set for zero: `BigDecimal` has no negative zero.
    negative: bool,
    /// The unscaled value's ASCII digits without leading zeros; `"0"` for zero.
    digits: String,
    scale: i64,
}

impl JavaDecimal {
    /// Parses what `new BigDecimal(String)` parses: an optional sign, digits with at most one decimal point, and an
    /// optional exponent (`12.50`, `+.5`, `5.`, `1E+2`, `125e-1`). `None` is Java's `NumberFormatException`.
    ///
    /// Like Java, every decimal digit of the Basic Multilingual Plane counts as a digit (`١٢` is 12), in the
    /// mantissa and in the exponent.
    pub(super) fn parse(text: &str) -> Option<Self> {
        let ascii: String = text.chars().map(to_ascii_digit).collect();
        let text = ascii.as_str();
        let (negative, unsigned) = match text.strip_prefix('-') {
            Some(rest) => (true, rest),
            None => (false, text.strip_prefix('+').unwrap_or(text)),
        };
        let (mantissa, exponent) = match unsigned.split_once(['e', 'E']) {
            Some((mantissa, exponent)) => (mantissa, parse_exponent(exponent)?),
            None => (unsigned, 0),
        };
        let (integer, fraction) = mantissa.split_once('.').unwrap_or((mantissa, ""));
        if integer.is_empty() && fraction.is_empty() {
            return None;
        }
        if !integer
            .bytes()
            .chain(fraction.bytes())
            .all(|byte| byte.is_ascii_digit())
        {
            return None;
        }
        let scale = i64::try_from(fraction.len()).ok()?.checked_sub(exponent)?;
        // Java keeps the scale in an `int` and refuses a number whose scale does not fit.
        i32::try_from(scale).ok()?;
        Some(Self::from_parts(negative, &format!("{integer}{fraction}"), scale))
    }

    /// The same number as the given `Decimal`, with the same scale.
    pub(super) fn from_decimal(value: Decimal) -> Self {
        let digits = value.mantissa().unsigned_abs().to_string();
        Self::from_parts(value.is_sign_negative(), &digits, i64::from(value.scale()))
    }

    fn from_parts(negative: bool, digits: &str, scale: i64) -> Self {
        let significant = digits.trim_start_matches('0');
        if significant.is_empty() {
            Self {
                negative: false,
                digits: "0".to_owned(),
                scale,
            }
        } else {
            Self {
                negative,
                digits: significant.to_owned(),
                scale,
            }
        }
    }

    /// `signum() > 0`.
    pub(super) fn is_positive(&self) -> bool {
        !self.negative && self.digits != "0"
    }

    /// The number of decimal places that are really used: `stripTrailingZeros().scale()`.
    /// Negative for a multiple of ten written with an exponent (`1E+2` has -2).
    pub(super) fn used_decimal_places(&self) -> i64 {
        let (_, scale) = self.without_trailing_zeros();
        scale
    }

    /// How many digits stand before the decimal point; zero or less for a number below 1.
    pub(super) fn integer_digits(&self) -> i64 {
        len_i64(&self.digits).saturating_sub(self.scale)
    }

    /// This number as a `Decimal` with exactly `scale` decimal places.
    ///
    /// `None` if it uses more decimal places than that, or is too large for a `Decimal`.
    pub(super) fn to_decimal_with_scale(&self, scale: u8) -> Option<Decimal> {
        let (digits, used_scale) = self.without_trailing_zeros();
        let missing_zeros = u32::try_from(i64::from(scale).checked_sub(used_scale)?).ok()?;
        let unscaled = digits
            .parse::<i128>()
            .ok()?
            .checked_mul(10_i128.checked_pow(missing_zeros)?)?;
        let unscaled = if self.negative {
            unscaled.checked_neg()?
        } else {
            unscaled
        };
        Decimal::try_from_i128_with_scale(unscaled, u32::from(scale)).ok()
    }

    /// The digits and scale after dropping trailing zeros. Zero stays as it is.
    fn without_trailing_zeros(&self) -> (&str, i64) {
        if self.digits == "0" {
            return (&self.digits, self.scale);
        }
        let stripped = self.digits.trim_end_matches('0');
        let dropped = len_i64(&self.digits).saturating_sub(len_i64(stripped));
        (stripped, self.scale.saturating_sub(dropped))
    }
}

/// The ASCII digit with the value of `c` if `c` is a decimal digit for Java's `Character.isDigit(char)`, else `c`.
pub(crate) fn to_ascii_digit(c: char) -> char {
    let Ok(unit) = u16::try_from(u32::from(c)) else {
        // Outside the BMP: two surrogates in Java, neither of them a digit.
        return c;
    };
    let run = ZEROS.partition_point(|zero| *zero <= unit).checked_sub(1);
    let value = run
        .and_then(|index| ZEROS.get(index))
        .and_then(|zero| unit.checked_sub(*zero))
        .filter(|value| *value < 10)
        .and_then(|value| u8::try_from(value).ok());
    value.map_or(c, |value| char::from(b'0'.saturating_add(value)))
}

/// The exponent after `e`/`E`: an optional sign and at least one digit, within Java's `int` range.
fn parse_exponent(text: &str) -> Option<i64> {
    let (negative, digits) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text.strip_prefix('+').unwrap_or(text)),
    };
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let significant = digits.trim_start_matches('0');
    // Ten digits are enough for any `int`; refusing more keeps the parse below from overflowing.
    if significant.len() > 10 {
        return None;
    }
    let magnitude: i64 = if significant.is_empty() {
        0
    } else {
        significant.parse().ok()?
    };
    let exponent = if negative { magnitude.checked_neg()? } else { magnitude };
    i32::try_from(exponent).ok().map(i64::from)
}

fn len_i64(text: &str) -> i64 {
    i64::try_from(text.len()).unwrap_or(i64::MAX)
}

/// Exactly `BigDecimal.toString()`: plain digits when the scale is not negative and the number is not tiny,
/// scientific notation otherwise (`1E+2`, `0E-7`, `1.2E-7`).
impl fmt::Display for JavaDecimal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.negative {
            f.write_str("-")?;
        }
        let digits = self.digits.as_str();
        let len = len_i64(digits);
        if self.scale == 0 {
            return f.write_str(digits);
        }
        // The exponent the number would have with one digit before the decimal point.
        let adjusted = len.saturating_sub(1).saturating_sub(self.scale);
        if self.scale > 0 && adjusted >= -6 {
            let integer_len = len.saturating_sub(self.scale);
            return match usize::try_from(integer_len)
                .ok()
                .filter(|&at| at > 0)
                .and_then(|at| digits.split_at_checked(at))
            {
                Some((integer, fraction)) => write!(f, "{integer}.{fraction}"),
                None => {
                    let leading_zeros = usize::try_from(integer_len.saturating_neg()).unwrap_or(0);
                    write!(f, "0.{}{digits}", "0".repeat(leading_zeros))
                }
            };
        }
        match digits.split_at_checked(1) {
            Some((first, rest)) if !rest.is_empty() => write!(f, "{first}.{rest}")?,
            _ => f.write_str(digits)?,
        }
        match adjusted {
            0 => Ok(()),
            positive if positive > 0 => write!(f, "E+{positive}"),
            negative => write!(f, "E{negative}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn printed(text: &str) -> String {
        JavaDecimal::parse(text).unwrap().to_string()
    }

    /// Input and the `new BigDecimal(input).toString()` Java gives for it.
    #[test]
    fn every_digit_of_every_run_maps_to_its_ascii_digit() {
        for zero in ZEROS {
            for value in 0..10_u16 {
                let c = char::from_u32(u32::from(zero + value)).unwrap();
                assert_eq!(
                    to_ascii_digit(c),
                    char::from(b'0' + u8::try_from(value).unwrap()),
                    "{c:?}"
                );
            }
        }
        assert_eq!(to_ascii_digit('a'), 'a');
        assert_eq!(to_ascii_digit('.'), '.');
        assert_eq!(to_ascii_digit('\u{1D7CE}'), '\u{1D7CE}');
        assert!(
            ZEROS.windows(2).all(|pair| pair[0] + 10 <= pair[1]),
            "sorted runs that do not overlap"
        );
    }

    #[test]
    fn prints_like_big_decimal_to_string() {
        let cases = [
            ("0", "0"),
            ("-0", "0"),
            ("0.00", "0.00"),
            ("12.50", "12.50"),
            ("-12.50", "-12.50"),
            ("+5", "5"),
            ("5.", "5"),
            (".5", "0.5"),
            ("005.10", "5.10"),
            ("0.05", "0.05"),
            ("123.456", "123.456"),
            ("100", "100"),
            ("1E+2", "1E+2"),
            ("1e2", "1E+2"),
            ("12E+2", "1.2E+3"),
            ("-1e2", "-1E+2"),
            ("1250E-2", "12.50"),
            ("1.5E1", "15"),
            ("1.5E0", "1.5"),
            ("15E-1", "1.5"),
            ("0E+2", "0E+2"),
            ("0.000001", "0.000001"),
            ("0.0000001", "1E-7"),
            ("0.0000000", "0E-7"),
            ("0.000000", "0.000000"),
            ("0.00000012", "1.2E-7"),
            ("123E-9", "1.23E-7"),
            ("1234567E-9", "0.001234567"),
            ("1E-400", "1E-400"),
            ("1E+400", "1E+400"),
            ("10E0", "10"),
            ("1E0", "1"),
            ("1E-0", "1"),
        ];
        for (input, expected) in cases {
            assert_eq!(printed(input), expected, "input {input:?}");
        }
    }

    #[test]
    fn rejects_what_big_decimal_rejects() {
        for text in [
            "",
            ".",
            "+",
            "-",
            "e1",
            "1e",
            "1e+",
            "1e1e1",
            "1.2.3",
            "1 ",
            " 1",
            "1_0",
            "--1",
            "1e2147483648",
        ] {
            assert_eq!(JavaDecimal::parse(text), None, "{text:?}");
        }
    }

    #[test]
    fn the_exponent_and_the_scale_must_fit_javas_int() {
        assert!(JavaDecimal::parse("1e2147483647").is_some());
        assert!(JavaDecimal::parse("1e-2147483647").is_some());
        assert!(JavaDecimal::parse("1e0000000000002147483647").is_some());
        assert_eq!(JavaDecimal::parse("1e-2147483648"), None);
        // scale = 1 - (-2147483647) does not fit an int
        assert_eq!(JavaDecimal::parse("1.5e-2147483647"), None);
    }

    #[test]
    fn counts_used_decimal_places_like_strip_trailing_zeros() {
        let places = |text: &str| JavaDecimal::parse(text).unwrap().used_decimal_places();
        assert_eq!(places("10.000"), -1);
        assert_eq!(places("11.000"), 0);
        assert_eq!(places("10.010"), 2);
        assert_eq!(places("1.5"), 1);
        assert_eq!(places("100"), -2);
        assert_eq!(places("1E+2"), -2);
        assert_eq!(places("0.00"), 2);
    }

    #[test]
    fn counts_the_digits_before_the_decimal_point() {
        let digits = |text: &str| JavaDecimal::parse(text).unwrap().integer_digits();
        assert_eq!(digits("1"), 1);
        assert_eq!(digits("999999999999999.99"), 15);
        assert_eq!(digits("1E+15"), 16);
        assert_eq!(digits("0.05"), -1);
        assert_eq!(digits("0012.5"), 2);
    }

    #[test]
    fn converts_to_a_decimal_with_the_wanted_scale() {
        let convert = |text: &str, scale: u8| {
            JavaDecimal::parse(text)
                .unwrap()
                .to_decimal_with_scale(scale)
                .map(|decimal| decimal.to_string())
        };
        assert_eq!(convert("12.5", 2).as_deref(), Some("12.50"));
        assert_eq!(convert("12.5000", 2).as_deref(), Some("12.50"));
        assert_eq!(convert("1E+2", 2).as_deref(), Some("100.00"));
        assert_eq!(convert("15", 0).as_deref(), Some("15"));
        assert_eq!(convert("-1.5", 1).as_deref(), Some("-1.5"));
        assert_eq!(convert("12.345", 2), None);
        assert_eq!(convert("1E+400", 2), None);
    }

    #[test]
    fn a_decimal_keeps_its_digits_and_scale() {
        assert_eq!(JavaDecimal::from_decimal(Decimal::new(1250, 2)).to_string(), "12.50");
        assert_eq!(JavaDecimal::from_decimal(Decimal::new(-5, 1)).to_string(), "-0.5");
        assert_eq!(JavaDecimal::from_decimal(Decimal::new(0, 2)).to_string(), "0.00");
        assert_eq!(JavaDecimal::from_decimal(-Decimal::ZERO).to_string(), "0");
        assert_eq!(JavaDecimal::from_decimal(Decimal::new(1, 7)).to_string(), "1E-7");
    }
}
