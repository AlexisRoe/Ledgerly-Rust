//! Lesson 51: conversion traits. Run `cargo test ex51`.

use std::fmt;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Currency {
    Eur,
    Usd,
}

#[derive(Debug, PartialEq)]
pub struct UnknownCurrency(pub String);

// 1. Display: Eur → "EUR", Usd → "USD".
impl fmt::Display for Currency {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Currency::Eur => "EUR",
            Currency::Usd => "USD",
        })
    }
}

// 2. FromStr: "EUR" → Eur, "USD" → Usd, anything else → UnknownCurrency(text).
impl FromStr for Currency {
    type Err = UnknownCurrency;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "EUR" => Ok(Currency::Eur),
            "USD" => Ok(Currency::Usd),
            other => Err(UnknownCurrency(other.to_owned())),
        }
    }
}

/// An amount in cents.
#[derive(Debug, PartialEq)]
pub struct Cents(pub u64);

// 3. From<u32> for Cents (lossless widening). Then `let c: Cents = 250_u32.into();` works.
impl From<u32> for Cents {
    fn from(value: u32) -> Self {
        Cents(u64::from(value))
    }
}

/// A strictly positive amount.
#[derive(Debug, PartialEq)]
pub struct Positive(pub u64);

// 4. TryFrom<i64> for Positive: values > 0 succeed, others fail with the message "must be positive".
impl TryFrom<i64> for Positive {
    type Error = &'static str;

    fn try_from(value: i64) -> Result<Self, Self::Error> {
        u64::try_from(value).ok().filter(|v| *v > 0).map(Positive).ok_or("must be positive")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_and_to_string() {
        assert_eq!(format!("{} {}", Currency::Eur, Currency::Usd), "EUR USD");
        assert_eq!(Currency::Eur.to_string(), "EUR");
    }

    #[test]
    fn parse() {
        assert_eq!("USD".parse::<Currency>(), Ok(Currency::Usd));
        assert_eq!("XYZ".parse::<Currency>(), Err(UnknownCurrency("XYZ".to_owned())));
    }

    #[test]
    fn into_cents() {
        let c: Cents = 250_u32.into();
        assert_eq!(c, Cents(250));
    }

    #[test]
    fn try_positive() {
        assert_eq!(Positive::try_from(5), Ok(Positive(5)));
        assert_eq!(Positive::try_from(0), Err("must be positive"));
        let p: Result<Positive, _> = (-3_i64).try_into();
        assert!(p.is_err());
    }
}
