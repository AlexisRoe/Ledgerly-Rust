//! The types of the service. Each one checks its value once, in its constructor, so a `Slug` or a `Target` that
//! exists is valid (parse, don't validate).

use std::fmt;

/// Why a value was refused. Every variant becomes a 400 response.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DomainError {
    #[error("a slug is {min} to {max} characters of a-z, 0-9 and -", min = Slug::MIN_LEN, max = Slug::MAX_LEN)]
    InvalidSlug,
    #[error("the slug {0:?} is reserved")]
    ReservedSlug(String),
    #[error("a target is an http or https URL of at most {max} characters, without spaces", max = Target::MAX_LEN)]
    InvalidTarget,
}

/// The short name in `/{slug}`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Slug(String);

impl Slug {
    pub const MIN_LEN: usize = 3;
    pub const MAX_LEN: usize = 32;
    /// Slugs that would shadow a route: `GET /links` must not become a redirect.
    const RESERVED: [&'static str; 1] = ["links"];

    /// # Errors
    ///
    /// [`DomainError::InvalidSlug`] for a wrong length or character, [`DomainError::ReservedSlug`] for a route name.
    pub fn parse(text: &str) -> Result<Self, DomainError> {
        let valid_char = |c: char| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-';
        if !(Self::MIN_LEN..=Self::MAX_LEN).contains(&text.len()) || !text.chars().all(valid_char) {
            return Err(DomainError::InvalidSlug);
        }
        if Self::RESERVED.contains(&text) {
            return Err(DomainError::ReservedSlug(text.to_owned()));
        }
        Ok(Self(text.to_owned()))
    }

    /// A slug made from a counter: the number in base 36, at least four characters long (`0000`, `0001`, …, `zzzz`,
    /// `10000`). Always valid, so no `Result`.
    #[must_use]
    pub fn generate(number: u64) -> Self {
        const DIGITS: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";
        let mut text = Vec::new();
        let mut rest = number;
        loop {
            text.push(DIGITS[(rest % 36) as usize]);
            rest /= 36;
            if rest == 0 {
                break;
            }
        }
        while text.len() < 4 {
            text.push(b'0');
        }
        text.reverse();
        Self(text.into_iter().map(char::from).collect())
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Slug {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Where a slug redirects to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target(String);

impl Target {
    pub const MAX_LEN: usize = 2048;

    /// Trims the text, then requires `http://` or `https://`, something after it, no whitespace, and at most
    /// [`Target::MAX_LEN`] bytes.
    ///
    /// # Errors
    ///
    /// [`DomainError::InvalidTarget`] otherwise.
    pub fn parse(text: &str) -> Result<Self, DomainError> {
        let text = text.trim();
        let rest = text.strip_prefix("https://").or_else(|| text.strip_prefix("http://"));
        match rest {
            Some(rest) if !rest.is_empty() && text.len() <= Self::MAX_LEN && !text.contains(char::is_whitespace) => {
                Ok(Self(text.to_owned()))
            }
            _ => Err(DomainError::InvalidTarget),
        }
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A stored link.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    pub slug: Slug,
    pub target: Target,
    pub visits: u64,
}

impl Link {
    #[must_use]
    pub const fn new(slug: Slug, target: Target) -> Self {
        Self {
            slug,
            target,
            visits: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugs_are_short_lowercase_words() {
        assert_eq!(
            Slug::parse("team-offsite").map(|s| s.to_string()),
            Ok("team-offsite".to_owned())
        );
        assert_eq!(Slug::parse("ab"), Err(DomainError::InvalidSlug));
        assert_eq!(Slug::parse("Team"), Err(DomainError::InvalidSlug));
        assert_eq!(Slug::parse("a b c"), Err(DomainError::InvalidSlug));
        assert_eq!(Slug::parse(&"x".repeat(33)), Err(DomainError::InvalidSlug));
    }

    #[test]
    fn route_names_are_reserved() {
        assert_eq!(Slug::parse("links"), Err(DomainError::ReservedSlug("links".to_owned())));
    }

    #[test]
    fn generated_slugs_count_in_base_36() {
        let slugs: Vec<String> = [0, 1, 35, 36, 36_u64.pow(4)]
            .map(|n| Slug::generate(n).to_string())
            .to_vec();
        assert_eq!(slugs, ["0000", "0001", "000z", "0010", "10000"]);
        for n in [0, 7, 1_000_000] {
            let generated = Slug::generate(n);
            assert_eq!(Slug::parse(generated.as_str()), Ok(generated));
        }
    }

    #[test]
    fn targets_are_http_urls() {
        assert_eq!(
            Target::parse(" https://example.org/a ").map(|t| t.as_str().to_owned()),
            Ok("https://example.org/a".to_owned())
        );
        assert!(Target::parse("http://x").is_ok());
        for bad in ["ftp://x", "https://", "example.org", "https://a b"] {
            assert_eq!(Target::parse(bad), Err(DomainError::InvalidTarget), "{bad}");
        }
        assert_eq!(
            Target::parse(&format!("https://{}", "a".repeat(2048))),
            Err(DomainError::InvalidTarget)
        );
    }
}
