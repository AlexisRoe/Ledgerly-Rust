//! Money: an amount together with its currency.
//!
//! Java counterpart: `shared/money/` plus the JDK's `java.util.Currency`, whose data is reproduced in the
//! generated `currencies` table.

#![cfg_attr(not(test), deny(clippy::indexing_slicing, clippy::arithmetic_side_effects))]

mod currencies;
mod currency;
pub(crate) mod decimal_text;
mod unicode_digits;

use std::cmp::Ordering;

use rust_decimal::Decimal;

pub use currency::{Currency, CurrencyError};
use decimal_text::JavaDecimal;

/// The most digits an amount may have before the decimal point: the `amount` column is `numeric(19,4)`.
const MAX_INTEGER_DIGITS: i64 = 15;

/// An amount together with its currency.
///
/// A `Money` is always positive, always carries exactly the currency's number of decimal places (`12.5 EUR` is
/// stored as `12.50`), and always fits the `numeric(19,4)` column. Because of the fixed scale, equal amounts are
/// equal values with equal hashes, however they were written.
///
/// `Money` has no `Ord` and no `+`: comparing or adding amounts in different currencies has no answer, so
/// [`Money::cmp`] and [`Money::checked_add`] return a `Result`. In Java, `Money` implements `Comparable` and
/// throws `CurrencyMismatch` at runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Money {
    amount: Decimal,
    currency: Currency,
}

/// Why something is not a [`Money`], or why two could not be combined.
///
/// The messages are Java's wherever Ledgerly-Java sends them to clients (port spec §3.2). `TooLarge` has no Java
/// counterpart: Java's `Money` accepts any size and fails later in the database.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MoneyError {
    /// The amount text is not a decimal number as Java's `BigDecimal` reads it.
    #[error("Not a valid decimal number")]
    NotADecimal,
    /// The currency code is not a currency amounts can be in.
    #[error(transparent)]
    Currency(#[from] CurrencyError),
    /// Zero or negative. `amount` is the rejected amount as Java prints it.
    #[error("Amount must be positive: {amount}")]
    NotPositive { amount: String },
    /// More decimal places than the currency has minor units, for example `10.001 EUR`.
    #[error("Amount {amount} has more than {minor_units} decimal places for {currency}")]
    TooManyDecimals {
        amount: String,
        minor_units: u8,
        currency: Currency,
    },
    /// Does not fit `numeric(19,4)`.
    #[error("Amount {amount} has more than 15 digits before the decimal point")]
    TooLarge { amount: String },
    /// Two amounts in different currencies were added or compared.
    #[error("Currency mismatch: {left} vs {right}")]
    CurrencyMismatch { left: Currency, right: Currency },
}

impl Money {
    /// An amount in a currency, checked and brought to the currency's scale.
    ///
    /// # Errors
    ///
    /// `NotPositive`, `TooManyDecimals` or `TooLarge`, checked in that order.
    pub fn new(amount: Decimal, currency: Currency) -> Result<Self, MoneyError> {
        Self::from_java_decimal(&JavaDecimal::from_decimal(amount), currency)
    }

    /// Parses an amount such as `"12.50"` and an ISO 4217 code such as `"EUR"`: Java's `Money.of(String, String)`.
    ///
    /// The amount may be written in any form Java's `BigDecimal` accepts, including `1E+2`.
    ///
    /// # Errors
    ///
    /// `NotADecimal` first, then a `Currency` error, then the rules of [`Money::new`]: the order in which Java
    /// evaluates `new Money(new BigDecimal(amount), Currency.getInstance(code))`.
    pub fn parse(amount: &str, currency_code: &str) -> Result<Self, MoneyError> {
        let amount = JavaDecimal::parse(amount).ok_or(MoneyError::NotADecimal)?;
        let currency = Currency::from_code(currency_code)?;
        Self::from_java_decimal(&amount, currency)
    }

    fn from_java_decimal(amount: &JavaDecimal, currency: Currency) -> Result<Self, MoneyError> {
        let minor_units = currency.minor_units();
        if !amount.is_positive() {
            return Err(MoneyError::NotPositive {
                amount: amount.to_string(),
            });
        }
        if amount.used_decimal_places() > i64::from(minor_units) {
            return Err(MoneyError::TooManyDecimals {
                amount: amount.to_string(),
                minor_units,
                currency,
            });
        }
        let too_large = || MoneyError::TooLarge {
            amount: amount.to_string(),
        };
        if amount.integer_digits() > MAX_INTEGER_DIGITS {
            return Err(too_large());
        }
        let amount = amount.to_decimal_with_scale(minor_units).ok_or_else(too_large)?;
        Ok(Self { amount, currency })
    }

    /// The amount, with exactly [`Currency::minor_units`] decimal places. Its `to_string()` is the API's form.
    #[must_use]
    pub const fn amount(&self) -> Decimal {
        self.amount
    }

    #[must_use]
    pub const fn currency(&self) -> Currency {
        self.currency
    }

    /// The sum of two amounts in the same currency: Java's `plus`.
    ///
    /// # Errors
    ///
    /// `CurrencyMismatch` if the currencies differ; `TooLarge` if the sum no longer fits the column.
    pub fn checked_add(&self, other: &Self) -> Result<Self, MoneyError> {
        self.require_same_currency(other)?;
        match self.amount.checked_add(other.amount) {
            Some(sum) => Self::new(sum, self.currency),
            // Unreachable with two valid amounts, which are far below `Decimal::MAX`; an error all the same.
            None => Err(MoneyError::TooLarge {
                amount: format!("{} + {}", self.amount, other.amount),
            }),
        }
    }

    /// Which of two amounts in the same currency is larger: Java's `compareTo`.
    ///
    /// This is deliberately not [`Ord::cmp`]: `Ord` promises an answer for every pair of values.
    ///
    /// # Errors
    ///
    /// `CurrencyMismatch` if the currencies differ.
    #[expect(
        clippy::should_implement_trait,
        reason = "the name is the point: this is the comparison `Ord::cmp` cannot be, because it can fail"
    )]
    pub fn cmp(&self, other: &Self) -> Result<Ordering, MoneyError> {
        self.require_same_currency(other)?;
        Ok(self.amount.cmp(&other.amount))
    }

    fn require_same_currency(&self, other: &Self) -> Result<(), MoneyError> {
        if self.currency == other.currency {
            Ok(())
        } else {
            Err(MoneyError::CurrencyMismatch {
                left: self.currency,
                right: other.currency,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cmp::Ordering;
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    use proptest::prelude::*;

    use super::currencies::CURRENCIES;
    use super::*;

    fn money(amount: &str, currency: &str) -> Money {
        Money::parse(amount, currency).unwrap()
    }

    fn error(amount: &str, currency: &str) -> MoneyError {
        Money::parse(amount, currency).unwrap_err()
    }

    fn hash_of(value: &Money) -> u64 {
        let mut hasher = DefaultHasher::new();
        value.hash(&mut hasher);
        hasher.finish()
    }

    // --- ported from Java's MoneyTest -----------------------------------------------------------------------

    #[test]
    fn rejects_zero_and_negative_amounts() {
        for amount in ["0", "0.00", "-1", "-0.01"] {
            let error = error(amount, "EUR");
            assert!(matches!(error, MoneyError::NotPositive { .. }), "{amount}: {error:?}");
            assert_eq!(error.to_string(), format!("Amount must be positive: {amount}"));
        }
    }

    #[test]
    fn rejects_scale_above_minor_units() {
        assert_eq!(
            error("10.001", "EUR").to_string(),
            "Amount 10.001 has more than 2 decimal places for EUR"
        );
        assert_eq!(
            error("1.5", "JPY").to_string(),
            "Amount 1.5 has more than 0 decimal places for JPY"
        );
        assert!(matches!(error("12.345", "EUR"), MoneyError::TooManyDecimals { .. }));
    }

    #[test]
    fn accepts_scale_up_to_minor_units() {
        assert_eq!(money("1.001", "KWD").amount().to_string(), "1.001");
        assert_eq!(money("1500", "JPY").amount().to_string(), "1500");
        assert_eq!(money("0.0001", "CLF").amount().to_string(), "0.0001");
    }

    #[test]
    fn trailing_zeros_beyond_minor_units_are_harmless() {
        assert_eq!(money("10.000", "EUR"), money("10.00", "EUR"));
        assert_eq!(money("10.000", "EUR").amount().to_string(), "10.00");
    }

    #[test]
    fn normalises_scale() {
        assert_eq!(money("10", "EUR").amount().scale(), 2);
        assert_eq!(money("10.5", "EUR").amount().to_string(), "10.50");
        assert_eq!(money("1E+2", "EUR").amount().to_string(), "100.00");
        assert_eq!(money("10", "JPY").amount().scale(), 0);
        assert_eq!(money("12.5", "KWD").amount().to_string(), "12.500");
    }

    #[test]
    fn equals_and_hash_ignore_input_scale() {
        let a = money("10.0", "EUR");
        let b = money("10.00", "EUR");
        assert_eq!(a, b);
        assert_eq!(hash_of(&a), hash_of(&b));
    }

    #[test]
    fn different_currencies_are_not_equal() {
        assert_ne!(money("10", "EUR"), money("10", "USD"));
    }

    #[test]
    fn rejects_pseudo_currencies() {
        for code in ["XAU", "XTS", "XXX"] {
            let error = error("1", code);
            assert_eq!(error, MoneyError::Currency(CurrencyError::NotMonetary(code.to_owned())));
            assert_eq!(error.to_string(), format!("Not a monetary currency: {code}"));
        }
    }

    #[test]
    fn checked_add_adds_same_currency() {
        let sum = money("10.10", "EUR").checked_add(&money("0.95", "EUR")).unwrap();
        assert_eq!(sum, money("11.05", "EUR"));
        assert_eq!(sum.amount().to_string(), "11.05");
    }

    #[test]
    fn checked_add_across_currencies_is_a_currency_mismatch() {
        let error = money("1", "EUR").checked_add(&money("1", "USD")).unwrap_err();
        assert!(matches!(error, MoneyError::CurrencyMismatch { .. }));
        assert_eq!(error.to_string(), "Currency mismatch: EUR vs USD");
    }

    #[test]
    fn cmp_orders_same_currency() {
        assert_eq!(money("1", "EUR").cmp(&money("2", "EUR")), Ok(Ordering::Less));
        assert_eq!(money("2", "EUR").cmp(&money("1", "EUR")), Ok(Ordering::Greater));
        assert_eq!(money("2.0", "EUR").cmp(&money("2", "EUR")), Ok(Ordering::Equal));
    }

    #[test]
    fn cmp_across_currencies_is_a_currency_mismatch() {
        let error = money("1", "EUR").cmp(&money("1", "USD")).unwrap_err();
        assert_eq!(error.to_string(), "Currency mismatch: EUR vs USD");
    }

    // --- parsing: Java's `new BigDecimal(String)` and `Currency.getInstance` -------------------------------

    #[test]
    fn what_big_decimal_does_not_parse_is_not_a_decimal() {
        let not_numbers = [
            "",
            " ",
            "abc",
            " 1",
            "1 ",
            "1,5",
            "1_000",
            "1.2.3",
            ".",
            "+",
            "-",
            "--1",
            "+-1",
            "1e",
            "1e+",
            "e5",
            "1e1.5",
            "0x10",
            "NaN",
            "Infinity",
            "1e99999999999",
        ];
        for text in not_numbers {
            let error = error(text, "EUR");
            assert_eq!(error, MoneyError::NotADecimal, "{text:?}");
            assert_eq!(error.to_string(), "Not a valid decimal number");
        }
    }

    /// Java's `BigDecimal` reads any decimal digit of the Basic Multilingual Plane: Arabic-Indic `١٢` is 12,
    /// fullwidth `１２.５` is 12.5, and they may be mixed with ASCII digits and appear in the exponent.
    #[test]
    fn non_ascii_decimal_digits_are_read_like_javas() {
        assert_eq!(money("١٢", "EUR"), money("12", "EUR"));
        assert_eq!(money("１２.５", "EUR"), money("12.5", "EUR"));
        assert_eq!(money("1٢.5", "EUR"), money("12.5", "EUR"));
        assert_eq!(money("1E٢", "EUR"), money("100", "EUR"));
        assert_eq!(money("१०.००", "INR"), money("10", "INR"));
        // The message prints the number as `BigDecimal.toString()` does: in ASCII digits.
        assert_eq!(error("٠", "EUR").to_string(), "Amount must be positive: 0");
    }

    /// A digit outside the Basic Multilingual Plane is two UTF-16 units in Java, and neither is a digit.
    #[test]
    fn supplementary_digits_are_not_digits_in_java() {
        assert_eq!(error("\u{1D7CF}", "EUR"), MoneyError::NotADecimal);
        assert_eq!(error("1\u{104A1}", "EUR"), MoneyError::NotADecimal);
    }

    #[test]
    fn every_form_big_decimal_parses_is_accepted() {
        assert_eq!(money("+5", "EUR"), money("5", "EUR"));
        assert_eq!(money("5.", "EUR"), money("5", "EUR"));
        assert_eq!(money(".5", "EUR"), money("0.50", "EUR"));
        assert_eq!(money("005.10", "EUR"), money("5.1", "EUR"));
        assert_eq!(money("1e2", "EUR"), money("100", "EUR"));
        assert_eq!(money("1.5E1", "JPY"), money("15", "JPY"));
        assert_eq!(money("1250E-2", "EUR"), money("12.5", "EUR"));
        assert_eq!(money("1E+014", "EUR"), money("100000000000000", "EUR"));
    }

    #[test]
    fn the_amount_is_checked_before_the_currency_as_in_java() {
        assert_eq!(error("abc", "nope"), MoneyError::NotADecimal);
        assert!(matches!(
            error("1", "nope"),
            MoneyError::Currency(CurrencyError::WrongLength(_))
        ));
        assert_eq!(
            error("-1", "ABC").to_string(),
            "The input currency code: \"ABC\" is not a valid ISO 4217 code"
        );
    }

    #[test]
    fn messages_show_the_amount_as_javas_big_decimal_to_string_does() {
        assert_eq!(error("-1e2", "EUR").to_string(), "Amount must be positive: -1E+2");
        assert_eq!(error("0.0000000", "EUR").to_string(), "Amount must be positive: 0E-7");
        assert_eq!(error("-0", "EUR").to_string(), "Amount must be positive: 0");
        assert_eq!(error("-.50", "EUR").to_string(), "Amount must be positive: -0.50");
        assert_eq!(
            error("+.5", "JPY").to_string(),
            "Amount 0.5 has more than 0 decimal places for JPY"
        );
        assert_eq!(
            error("0.00000012", "EUR").to_string(),
            "Amount 1.2E-7 has more than 2 decimal places for EUR"
        );
        assert_eq!(
            error("1E-400", "EUR").to_string(),
            "Amount 1E-400 has more than 2 decimal places for EUR"
        );
    }

    // --- the column type: numeric(19,4) holds at most 15 digits before the decimal point --------------------

    #[test]
    fn the_largest_amount_the_column_holds_is_accepted() {
        assert_eq!(
            money("999999999999999.99", "EUR").amount().to_string(),
            "999999999999999.99"
        );
        assert_eq!(
            money("999999999999999.9999", "CLF").amount().to_string(),
            "999999999999999.9999"
        );
        assert_eq!(money("999999999999999", "JPY").amount().to_string(), "999999999999999");
    }

    #[test]
    fn amounts_that_do_not_fit_the_column_are_rejected() {
        for amount in [
            "1000000000000000",
            "1000000000000000.00",
            "1E+15",
            "1E+400",
            "99999999999999999999999999999999999999999",
        ] {
            let error = error(amount, "EUR");
            assert!(matches!(error, MoneyError::TooLarge { .. }), "{amount}: {error:?}");
        }
        assert_eq!(
            error("1000000000000000", "EUR").to_string(),
            "Amount 1000000000000000 has more than 15 digits before the decimal point"
        );
    }

    #[test]
    fn a_sum_that_does_not_fit_the_column_is_rejected() {
        let error = money("999999999999999.99", "EUR")
            .checked_add(&money("0.01", "EUR"))
            .unwrap_err();
        assert!(matches!(error, MoneyError::TooLarge { .. }));
    }

    // --- constructing from a Decimal (what persistence will do) ---------------------------------------------

    #[test]
    fn new_applies_the_same_rules_as_parse() {
        let eur = Currency::from_code("EUR").unwrap();
        assert_eq!(Money::new(Decimal::new(125, 1), eur).unwrap(), money("12.50", "EUR"));
        assert_eq!(Money::new(Decimal::new(125, 1), eur).unwrap().amount().scale(), 2);
        assert_eq!(
            Money::new(Decimal::new(12_5000, 4), eur).unwrap().amount().to_string(),
            "12.50"
        );
        assert!(matches!(
            Money::new(Decimal::ZERO, eur),
            Err(MoneyError::NotPositive { .. })
        ));
        assert!(matches!(
            Money::new(Decimal::new(-1, 2), eur),
            Err(MoneyError::NotPositive { .. })
        ));
        assert!(matches!(
            Money::new(Decimal::new(12_345, 3), eur),
            Err(MoneyError::TooManyDecimals { .. })
        ));
        assert!(matches!(
            Money::new(Decimal::MAX, eur),
            Err(MoneyError::TooLarge { .. })
        ));
    }

    #[test]
    fn accessors_return_the_parts() {
        let m = money("12.50", "EUR");
        assert_eq!(m.amount(), Decimal::new(1250, 2));
        assert_eq!(m.currency(), Currency::from_code("EUR").unwrap());
    }

    // --- properties ------------------------------------------------------------------------------------------

    /// Any currency amounts can be in.
    fn any_currency() -> impl Strategy<Value = Currency> {
        let monetary: Vec<Currency> = CURRENCIES
            .iter()
            .filter_map(|(code, _)| Currency::from_code(code).ok())
            .collect();
        prop::sample::select(monetary)
    }

    /// Any valid `Money`: 1 minor unit up to the largest amount `numeric(19,4)` holds in that currency.
    fn any_money() -> impl Strategy<Value = Money> {
        any_currency().prop_flat_map(|currency| {
            let scale = u32::from(currency.minor_units());
            let largest = 10_i128.pow(15 + scale) - 1;
            (1..=largest).prop_map(move |minor_units| {
                Money::new(Decimal::from_i128_with_scale(minor_units, scale), currency).unwrap()
            })
        })
    }

    proptest! {
        #[test]
        fn any_accepted_money_round_trips_through_its_string_form_unchanged(original in any_money()) {
            let amount = original.amount().to_string();
            let parsed = Money::parse(&amount, original.currency().code()).unwrap();
            prop_assert_eq!(parsed, original);
            prop_assert_eq!(parsed.amount().to_string(), amount);
            prop_assert_eq!(parsed.amount().scale(), u32::from(original.currency().minor_units()));
        }

        #[test]
        fn extra_trailing_zeros_never_change_the_value(original in any_money(), zeros in 1_usize..6) {
            let amount = original.amount().to_string();
            let separator = if amount.contains('.') { "" } else { "." };
            let padded = format!("{amount}{separator}{}", "0".repeat(zeros));
            let parsed = Money::parse(&padded, original.currency().code()).unwrap();
            prop_assert_eq!(parsed, original);
            prop_assert_eq!(hash_of(&parsed), hash_of(&original));
        }

        #[test]
        fn adding_is_commutative_and_keeps_the_scale(a in any_money(), b in any_money()) {
            match (a.checked_add(&b), b.checked_add(&a)) {
                (Ok(left), Ok(right)) => {
                    prop_assert_eq!(left, right);
                    prop_assert_eq!(left.amount().scale(), u32::from(a.currency().minor_units()));
                    prop_assert_eq!(left.cmp(&a), Ok(Ordering::Greater));
                }
                (Err(left), Err(_)) => {
                    let mismatch = a.currency() != b.currency();
                    prop_assert_eq!(matches!(left, MoneyError::CurrencyMismatch { .. }), mismatch);
                    prop_assert_eq!(matches!(left, MoneyError::TooLarge { .. }), !mismatch);
                }
                (left, right) => prop_assert!(false, "{left:?} but {right:?}"),
            }
        }

        #[test]
        fn parsing_arbitrary_text_never_panics(amount in "\\PC{0,40}", currency in "\\PC{0,5}") {
            let _ = Money::parse(&amount, &currency);
        }

        #[test]
        fn parsing_number_like_text_never_panics(amount in "[-+]?[0-9]{0,40}\\.?[0-9]{0,40}([eE][-+]?[0-9]{1,12})?") {
            let _ = Money::parse(&amount, "EUR");
        }
    }
}
