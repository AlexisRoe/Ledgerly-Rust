//! Lesson 26: newtypes. Run `cargo test ex26`.

/// An email address that is known to contain exactly one '@' with text on both sides.
/// The field is private to this module: outside code must go through `parse`.
#[derive(Debug, Clone, PartialEq)]
pub struct Email(String);

impl Email {
    /// Accepts "a@b" shapes only: exactly one '@', not at the start or end. Stores the text lower-cased.
    /// Hints: `text.matches('@').count()`, `text.starts_with('@')`, `text.ends_with('@')`, `text.to_lowercase()`.
    pub fn parse(text: &str) -> Option<Self> {
        let ok = text.matches('@').count() == 1 && !text.starts_with('@') && !text.ends_with('@');
        if ok { Some(Self(text.to_lowercase())) } else { None }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// 1. Make `UserId` a newtype around u64 (a tuple struct), deriving Debug, Clone, Copy, PartialEq.
///    Replace this type alias, which offers no protection at all:
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UserId(pub u64);

/// 2. A function that used to take (u64, String) and was called the wrong way round in production.
///    Now it takes a UserId and an Email, so the compiler stops the mix-up.
pub fn invite(user: UserId, email: &Email) -> String {
    format!("user {:?} invited {}", user, email.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_accepts_and_normalises() {
        assert_eq!(Email::parse("Alex@Example.org").map(|e| e.as_str().to_owned()), Some("alex@example.org".to_owned()));
    }

    #[test]
    fn parse_rejects_bad_shapes() {
        for bad in ["no-at-sign", "@start", "end@", "two@@ats", "a@b@c"] {
            assert_eq!(Email::parse(bad), None, "{bad}");
        }
    }

    #[test]
    fn invite_uses_both_types() {
        let email = Email::parse("sam@example.org").unwrap();
        // 3. After step 1, change `7` below into `UserId(7)`.
        //    Then try swapping the arguments and read the error.
        let text = invite(UserId(7), &email);
        assert!(text.ends_with("invited sam@example.org"));
    }
}
