//! Whether a text is an email address, as Bean Validation's `@Email` decides it.
//!
//! Java counterpart: the `@Email` that openapi-generator puts on every `format: email` property, checked by
//! Hibernate Validator's `AbstractEmailValidator` and `DomainNameUtil` (Hibernate Validator 9, which Spring Boot
//! 4.2 ships). The patterns below are copied from there, so the same texts pass and fail:
//!
//! - split at the **last** `@`; no `@` at all fails;
//! - the local part has at most 64 characters and is dot-separated words, each either unquoted (letters,
//!   digits, ``!#$%&'*+/=?^_`{|}~-`` and anything from U+0080 to U+FFFF) or quoted;
//! - the domain does not end with a dot, has at most 255 characters in its ASCII (Punycode) form, and is
//!   dot-separated labels, an IPv4 literal `[1.2.3.4]` or an IPv6 literal `[IPv6:…]`.
//!
//! The empty text passes, as in Hibernate: `@NotNull` and `@Size` are separate constraints.
//!
//! One difference is left: Java converts a non-ASCII domain with `java.net.IDN` (IDNA2003), this with the
//! `idna` crate (UTS 46). Both refuse a label over 63 characters and characters IDNA prohibits; they can
//! disagree about exotic characters that one standard maps and the other prohibits.

use std::sync::LazyLock;

use idna::AsciiDenyList;
use regex::Regex;

use crate::text::utf16_len;

/// `AbstractEmailValidator.MAX_LOCAL_PART_LENGTH`.
const MAX_LOCAL_PART_LENGTH: usize = 64;
/// `DomainNameUtil.MAX_DOMAIN_PART_LENGTH`.
const MAX_DOMAIN_PART_LENGTH: usize = 255;
/// What `java.net.IDN.toASCII` refuses: "The label in the input is too long".
const MAX_LABEL_LENGTH: usize = 63;

const LOCAL_PART_ATOM: &str = r"[a-z0-9!#$%&'*+/=?^_`{|}~\x{80}-\x{FFFF}-]";
const LOCAL_PART_INSIDE_QUOTES_ATOM: &str = r#"(?:[a-z0-9!#$%&'*.(),<>\[\]:;  @+/=?^_`{|}~\x{80}-\x{FFFF}-]|\\\\|\\")"#;
const DOMAIN_CHARS_WITHOUT_DASH: &str = r"[a-z\x{80}-\x{FFFF}0-9!#$%&'*+/=?^_`{|}~]";
const IP_DOMAIN: &str = r"[0-9]{1,3}\.[0-9]{1,3}\.[0-9]{1,3}\.[0-9]{1,3}";
const IP_V6_DOMAIN: &str = r"(?:(?:[0-9a-fA-F]{1,4}:){7,7}[0-9a-fA-F]{1,4}|(?:[0-9a-fA-F]{1,4}:){1,7}:|(?:[0-9a-fA-F]{1,4}:){1,6}:[0-9a-fA-F]{1,4}|(?:[0-9a-fA-F]{1,4}:){1,5}(?::[0-9a-fA-F]{1,4}){1,2}|(?:[0-9a-fA-F]{1,4}:){1,4}(?::[0-9a-fA-F]{1,4}){1,3}|(?:[0-9a-fA-F]{1,4}:){1,3}(?::[0-9a-fA-F]{1,4}){1,4}|(?:[0-9a-fA-F]{1,4}:){1,2}(?::[0-9a-fA-F]{1,4}){1,5}|[0-9a-fA-F]{1,4}:(?:(?::[0-9a-fA-F]{1,4}){1,6})|:(?:(?::[0-9a-fA-F]{1,4}){1,7}|:)|fe80:(?::[0-9a-fA-F]{0,4}){0,4}%[0-9a-zA-Z]{1,}|::(?:ffff(:0{1,4}){0,1}:){0,1}(?:(?:25[0-5]|(?:2[0-4]|1{0,1}[0-9]){0,1}[0-9])\.){3,3}(?:25[0-5]|(?:2[0-4]|1{0,1}[0-9]){0,1}[0-9])|(?:[0-9a-fA-F]{1,4}:){1,4}:(?:(?:25[0-5]|(?:2[0-4]|1{0,1}[0-9]){0,1}[0-9])\.){3,3}(?:25[0-5]|(?:2[0-4]|1{0,1}[0-9]){0,1}[0-9]))";

/// `LOCAL_PART_PATTERN`, case-insensitive, matched against the whole local part.
static LOCAL_PART: LazyLock<Option<Regex>> = LazyLock::new(|| {
    let word = format!(r#"(?:{LOCAL_PART_ATOM}+|"{LOCAL_PART_INSIDE_QUOTES_ATOM}+")"#);
    Regex::new(&format!(r"(?i)\A{word}(?:\.{word})*\z")).ok()
});

/// `EMAIL_DOMAIN_PATTERN`, case-insensitive, matched against the whole domain. Java's possessive quantifiers
/// (`++`, `*+`) change nothing here: dashes and the other characters never overlap.
static DOMAIN: LazyLock<Option<Regex>> = LazyLock::new(|| {
    let label = format!("{DOMAIN_CHARS_WITHOUT_DASH}+(?:-+{DOMAIN_CHARS_WITHOUT_DASH}+)*");
    let domain = format!(r"{label}(?:\.{label})*");
    Regex::new(&format!(
        r"(?i)\A(?:{domain}|\[{IP_DOMAIN}\]|\[IPv6:{IP_V6_DOMAIN}\])\z"
    ))
    .ok()
});

/// Hibernate's `@Email` check. See the module documentation for the rules.
#[must_use]
pub fn is_well_formed(text: &str) -> bool {
    if text.is_empty() {
        return true;
    }
    let Some((local_part, domain)) = text.rsplit_once('@') else {
        return false;
    };
    is_local_part(local_part) && is_domain(domain)
}

fn is_local_part(local_part: &str) -> bool {
    utf16_len(local_part) <= MAX_LOCAL_PART_LENGTH
        && LOCAL_PART.as_ref().is_some_and(|pattern| pattern.is_match(local_part))
}

/// `DomainNameUtil.isValidEmailDomainAddress`.
fn is_domain(domain: &str) -> bool {
    if domain.ends_with('.') {
        return false;
    }
    let Some(ascii) = to_ascii(domain) else {
        return false;
    };
    ascii.len() <= MAX_DOMAIN_PART_LENGTH && DOMAIN.as_ref().is_some_and(|pattern| pattern.is_match(domain))
}

/// `java.net.IDN.toASCII(domain)` with no flags: ASCII labels as they are, others converted to Punycode, any
/// label over 63 characters refused.
fn to_ascii(domain: &str) -> Option<String> {
    let ascii = if domain.is_ascii() {
        domain.to_owned()
    } else {
        idna::domain_to_ascii_cow(domain.as_bytes(), AsciiDenyList::EMPTY)
            .ok()?
            .into_owned()
    };
    ascii
        .split('.')
        .all(|label| label.len() <= MAX_LABEL_LENGTH)
        .then_some(ascii)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_patterns_compile() {
        assert!(LOCAL_PART.is_some());
        assert!(DOMAIN.is_some());
    }

    /// From Hibernate Validator's `EmailValidatorTest` and `AbstractEmailValidatorTest`.
    #[test]
    fn addresses_hibernate_accepts() {
        for address in [
            "",
            "alice@example.com",
            "Alice.Example@Example.COM",
            "user-2fd7c0e8@example.com",
            "first.last@sub.example.co.uk",
            "a+b@example.com",
            "!#$%&'*+/=?^_`{|}~-@example.com",
            "\"quoted string\"@example.com",
            "\"with \\\" quote\"@example.com",
            "\"with \\\\ backslash\"@example.com",
            "\"a@b\"@example.com",
            "user@localhost",
            "user@[127.0.0.1]",
            "user@[999.999.999.999]",
            "user@[IPv6:2001:db8::1]",
            "user@[ipv6:::1]",
            "user@xn--bcher-kva.de",
            "user@bücher.de",
            "jörg@example.com",
            "user@a--b.example",
            "user@123.example",
            "user@example.com_",
        ] {
            assert!(is_well_formed(address), "{address}");
        }
    }

    #[test]
    fn addresses_hibernate_refuses() {
        for address in [
            "not-an-email",
            "@example.com",
            "alice@",
            "alice@example.com.",
            "alice@.example.com",
            "alice@example..com",
            "alice@-example.com",
            "alice@example-.com",
            ".alice@example.com",
            "alice.@example.com",
            "al..ice@example.com",
            "al ice@example.com",
            " alice@example.com",
            "alice@example.com ",
            "\"unterminated@example.com",
            "\"\"@example.com",
            "\"bad \\x escape\"@example.com",
            "alice@[127.0.0.1",
            "alice@[1.2.3]",
            "alice@[IPv6:12345::1]",
            "alice@exa mple.com",
            "alice@example@",
            "😀@example.com",
        ] {
            assert!(!is_well_formed(address), "{address}");
        }
    }

    #[test]
    fn the_local_part_has_at_most_64_characters() {
        assert!(is_well_formed(&format!("{}@example.com", "a".repeat(64))));
        assert!(!is_well_formed(&format!("{}@example.com", "a".repeat(65))));
    }

    #[test]
    fn a_label_has_at_most_63_characters_and_the_domain_255() {
        assert!(is_well_formed(&format!("a@{}.com", "b".repeat(63))));
        assert!(!is_well_formed(&format!("a@{}.com", "b".repeat(64))));

        let label = "c".repeat(63);
        let long = [label.as_str(); 4].join(".");
        assert_eq!(long.len(), 255);
        assert!(is_well_formed(&format!("a@{long}")));
        assert!(!is_well_formed(&format!("a@{long}d")));
    }

    #[test]
    fn the_split_is_at_the_last_at_sign() {
        // "a@b" is not a valid unquoted local part, so this fails on the local part, not on the domain.
        assert!(!is_well_formed("a@b@example.com"));
        assert!(is_well_formed("\"a@b\"@example.com"));
    }
}
