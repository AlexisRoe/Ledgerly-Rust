//! Lesson 112: parse, don't validate, in depth. Run `cargo test ex112`.

use serde::Deserialize;

/// An email address that has been checked and normalised. The field is PRIVATE: outside this module, the only
/// way to get an `Email` is `Email::parse`, so every `Email` in the program is valid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Email(String);

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
#[error("not an email address: {0:?}")]
pub struct InvalidEmail(pub String);

impl Email {
    /// 1. Trim, lowercase, and accept only text with exactly one '@' that has something on both sides.
    ///    (A deliberately small rule for the exercise.)
    pub fn parse(text: &str) -> Result<Self, InvalidEmail> {
        let normalised = text.trim().to_lowercase();
        match normalised.split_once('@') {
            Some((local, domain)) if !local.is_empty() && !domain.is_empty() && !domain.contains('@') => {
                Ok(Self(normalised))
            }
            _ => Err(InvalidEmail(text.to_owned())),
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// 2. The part after '@'. No `Option` needed: a parsed Email always has one. Use `split_once` and fall
    ///    back to "" with `map_or` (it can't happen, and the compiler doesn't need an `unwrap`).
    pub fn domain(&self) -> &str {
        self.0.split_once('@').map_or("", |(_, domain)| domain)
    }
}

/// 3. Let serde go through the same door: `TryFrom<String>` plus `#[serde(try_from = "String")]` on the type
///    means JSON can't create an invalid Email either. Implement `try_from` by calling `Email::parse`.
impl TryFrom<String> for Email {
    type Error = InvalidEmail;

    fn try_from(text: String) -> Result<Self, InvalidEmail> {
        Email::parse(&text)
    }
}

/// The request body. Its `email` field is an `Email`, so deserialising checks it.
#[derive(Debug, Deserialize)]
pub struct Invite {
    pub email: SerdeEmail,
}

/// A wrapper so the serde attribute can be shown on its own line. In real code you'd put
/// `#[derive(Deserialize)] #[serde(try_from = "String")]` directly on `Email`.
#[derive(Debug, Deserialize)]
#[serde(try_from = "String")]
pub struct SerdeEmail(pub Email);

impl TryFrom<String> for SerdeEmail {
    type Error = InvalidEmail;

    fn try_from(text: String) -> Result<Self, InvalidEmail> {
        Email::try_from(text).map(SerdeEmail)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_normalises() {
        let email = Email::parse("  Sam@Example.ORG ");
        assert_eq!(email.as_ref().map(Email::as_str), Ok("sam@example.org"));
        assert_eq!(email.map(|e| e.domain().to_owned()), Ok("example.org".to_owned()));
        assert!(Email::parse("no-at-sign").is_err());
        assert!(Email::parse("@example.org").is_err());
        assert!(Email::parse("a@b@c").is_err());
    }

    #[test]
    fn json_goes_through_the_same_check() {
        let ok: Result<Invite, _> = serde_json::from_str(r#"{"email":"Ana@Example.org"}"#);
        assert_eq!(ok.map(|i| i.email.0.as_str().to_owned()).ok().as_deref(), Some("ana@example.org"));
        let bad: Result<Invite, _> = serde_json::from_str(r#"{"email":"nope"}"#);
        assert!(bad.is_err());
    }
}
