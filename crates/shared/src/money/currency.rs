use std::fmt;
use std::str::FromStr;

use super::currencies::CURRENCIES;

/// A currency that amounts of money can be in: an ISO 4217 code together with its number of minor-unit digits.
///
/// Replaces `java.util.Currency` as Ledgerly-Java uses it. Java accepts any code the JDK knows and `Money` then
/// refuses the ones without minor units (XAU, XTS, XXX, ...). Here such a code never becomes a `Currency`, so
/// `minor_units` needs no "undefined" case.
///
/// Two currencies are equal when their codes are equal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Currency {
    code: &'static str,
    minor_units: u8,
}

/// Why a string is not a [`Currency`].
///
/// The messages are the ones Ledgerly-Java ends up sending to clients: the first three are the
/// `IllegalArgumentException` messages of `java.util.Currency.getInstance` in OpenJDK 25, the last one is
/// `Money`'s own.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CurrencyError {
    /// Not three characters long, counted as Java counts them (UTF-16 units).
    #[error("The input currency code: \"{0}\" must have a length of 3 characters")]
    WrongLength(String),
    /// The first two characters are not both `A`–`Z`. Java looks them up as a country code first, hence the
    /// wording. Carries just those two characters.
    #[error("The country code: \"{0}\" is not a valid ISO 3166 code")]
    InvalidCountryPrefix(String),
    /// Three characters, but no ISO 4217 code that Java knows.
    #[error("The input currency code: \"{0}\" is not a valid ISO 4217 code")]
    Unknown(String),
    /// A real ISO 4217 code that has no minor units, so no amount of money can be in it (XAU, XTS, XXX, ...).
    #[error("Not a monetary currency: {0}")]
    NotMonetary(String),
}

impl Currency {
    /// Looks up an ISO 4217 code. Codes are case-sensitive: `eur` is not a currency.
    ///
    /// # Errors
    ///
    /// Returns a [`CurrencyError`] for anything `java.util.Currency.getInstance` rejects, checked in the same
    /// order, and for codes without minor units.
    pub fn from_code(code: &str) -> Result<Self, CurrencyError> {
        if code.encode_utf16().count() != 3 {
            return Err(CurrencyError::WrongLength(code.to_owned()));
        }
        let prefix: Vec<u16> = code.encode_utf16().take(2).collect();
        if !prefix
            .iter()
            .all(|unit| (u16::from(b'A')..=u16::from(b'Z')).contains(unit))
        {
            return Err(CurrencyError::InvalidCountryPrefix(String::from_utf16_lossy(&prefix)));
        }
        let entry = CURRENCIES
            .binary_search_by(|(known, _)| (*known).cmp(code))
            .ok()
            .and_then(|index| CURRENCIES.get(index));
        match entry {
            None => Err(CurrencyError::Unknown(code.to_owned())),
            Some((known, None)) => Err(CurrencyError::NotMonetary((*known).to_owned())),
            Some((known, Some(minor_units))) => Ok(Self {
                code: known,
                minor_units: *minor_units,
            }),
        }
    }

    /// The ISO 4217 code, such as `EUR`.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        self.code
    }

    /// How many decimal places an amount in this currency has: 2 for EUR, 0 for JPY, 3 for KWD.
    ///
    /// The same number as `java.util.Currency.getDefaultFractionDigits()`.
    #[must_use]
    pub const fn minor_units(&self) -> u8 {
        self.minor_units
    }
}

/// The ISO 4217 code, like `toString()` of Java's `Currency`.
impl fmt::Display for Currency {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code)
    }
}

impl FromStr for Currency {
    type Err = CurrencyError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::from_code(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn minor_units(code: &str) -> u8 {
        Currency::from_code(code).unwrap().minor_units()
    }

    #[test]
    fn minor_units_are_what_javas_currency_reports() {
        assert_eq!(minor_units("EUR"), 2);
        assert_eq!(minor_units("USD"), 2);
        assert_eq!(minor_units("JPY"), 0);
        assert_eq!(minor_units("KWD"), 3);
        assert_eq!(minor_units("CLF"), 4);
    }

    #[test]
    fn withdrawn_codes_are_still_accepted_because_java_accepts_them() {
        assert_eq!(minor_units("DEM"), 2);
        assert_eq!(minor_units("ITL"), 0);
    }

    #[test]
    fn codes_without_minor_units_are_rejected_with_javas_message() {
        for code in ["XAU", "XTS", "XXX"] {
            let error = Currency::from_code(code).unwrap_err();
            assert_eq!(error, CurrencyError::NotMonetary(code.to_owned()));
            assert_eq!(error.to_string(), format!("Not a monetary currency: {code}"));
        }
    }

    #[test]
    fn unknown_codes_are_rejected_with_the_jdks_message() {
        for code in ["ABC", "EU1", "EUr", "EU€"] {
            let error = Currency::from_code(code).unwrap_err();
            assert_eq!(
                error.to_string(),
                format!("The input currency code: \"{code}\" is not a valid ISO 4217 code"),
            );
        }
    }

    #[test]
    fn codes_are_case_sensitive_like_javas() {
        let error = Currency::from_code("eur").unwrap_err();
        assert_eq!(error, CurrencyError::InvalidCountryPrefix("eu".to_owned()));
        assert_eq!(
            error.to_string(),
            "The country code: \"eu\" is not a valid ISO 3166 code"
        );
    }

    #[test]
    fn the_first_two_characters_must_be_a_to_z() {
        for (code, prefix) in [("ÄÖÜ", "ÄÖ"), ("E1R", "E1"), ("1UR", "1U")] {
            let error = Currency::from_code(code).unwrap_err();
            assert_eq!(
                error.to_string(),
                format!("The country code: \"{prefix}\" is not a valid ISO 3166 code")
            );
        }
    }

    #[test]
    fn codes_must_have_three_characters() {
        for code in ["", "EU", "EURO", " EUR"] {
            let error = Currency::from_code(code).unwrap_err();
            assert_eq!(
                error.to_string(),
                format!("The input currency code: \"{code}\" must have a length of 3 characters"),
            );
        }
    }

    #[test]
    fn length_is_counted_in_utf16_units_like_javas_string_length() {
        // One character outside the Basic Multilingual Plane is two UTF-16 units, so "😀" plus "A" has "length 3" in
        // Java and gets past the length check.
        let error = Currency::from_code("😀A").unwrap_err();
        assert_eq!(
            error.to_string(),
            "The country code: \"😀\" is not a valid ISO 3166 code"
        );
    }

    #[test]
    fn display_and_from_str_use_the_code() {
        let eur: Currency = "EUR".parse().unwrap();
        assert_eq!(eur.code(), "EUR");
        assert_eq!(eur.to_string(), "EUR");
    }

    #[test]
    fn equal_codes_are_equal_currencies() {
        assert_eq!(Currency::from_code("EUR").unwrap(), Currency::from_code("EUR").unwrap());
        assert_ne!(Currency::from_code("EUR").unwrap(), Currency::from_code("USD").unwrap());
    }

    #[test]
    fn the_generated_table_is_sorted_and_free_of_duplicates() {
        // `Currency::from_code` finds codes by binary search, which needs exactly this.
        assert!(CURRENCIES.windows(2).all(|pair| pair[0].0 < pair[1].0));
    }

    #[test]
    fn the_generated_table_has_only_three_letter_upper_case_codes_and_sane_digits() {
        for (code, digits) in CURRENCIES {
            assert!(
                code.len() == 3 && code.bytes().all(|b| b.is_ascii_uppercase()),
                "{code}"
            );
            assert!(
                digits.is_none_or(|d| d <= 4),
                "{code} has {digits:?} fraction digits; numeric(19,4) holds 4"
            );
        }
    }
}
