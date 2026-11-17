//! Random bearer secrets: the refresh token and the email-verification token.
//!
//! Java counterpart: `identity/internal/application/OpaqueTokens.java`. A token is 32 random bytes as unpadded
//! Base64url (43 characters, 256 bits). Only its SHA-256 is stored, as lower-case hex of the token's UTF-8 bytes
//! (`refresh_tokens.token_hash`, `email_verifications.token_hash`), so a copy of the database holds no usable
//! token.

use std::fmt;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use secrecy::{ExposeSecret, SecretString};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use zeroize::Zeroizing;

/// Why no token could be made: the operating system's random number generator failed.
#[derive(Debug, thiserror::Error)]
#[error("no random bytes for a token")]
pub(crate) struct NoRandomness;

/// A fresh token for the client. Java: `OpaqueTokens.generate`.
pub(crate) fn generate() -> Result<SecretString, NoRandomness> {
    let mut bytes = Zeroizing::new([0_u8; 32]);
    getrandom::fill(bytes.as_mut_slice()).map_err(|_| NoRandomness)?;
    Ok(SecretString::from(URL_SAFE_NO_PAD.encode(bytes.as_slice())))
}

/// What is stored and looked up instead of a token.
///
/// Equality takes the same time wherever two hashes differ.
#[derive(Clone, Eq)]
pub(crate) struct TokenHash(String);

impl TokenHash {
    /// The hash of `token`, whatever the client sent. Java: `OpaqueTokens.hash`.
    pub(crate) fn of(token: &SecretString) -> Self {
        let digest = Sha256::digest(token.expose_secret().as_bytes());
        Self(digest.iter().map(|byte| format!("{byte:02x}")).collect())
    }

    /// The 64 hex characters.
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

impl PartialEq for TokenHash {
    fn eq(&self, other: &Self) -> bool {
        self.0.as_bytes().ct_eq(other.0.as_bytes()).into()
    }
}

/// A hash is not the token, but it finds the row of one: it is not printed either.
impl fmt::Debug for TokenHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("TokenHash(***)")
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use base64::engine::general_purpose::URL_SAFE_NO_PAD;

    use super::*;

    #[test]
    fn a_token_is_43_characters_of_base64url_for_32_bytes() {
        let token = generate().unwrap();
        let text = token.expose_secret();

        assert_eq!(text.len(), 43);
        assert!(
            text.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'),
            "{text}"
        );
        assert_eq!(URL_SAFE_NO_PAD.decode(text).unwrap().len(), 32);
    }

    #[test]
    fn tokens_do_not_repeat() {
        let tokens: HashSet<String> = (0..1000)
            .map(|_| generate().unwrap().expose_secret().to_owned())
            .collect();

        assert_eq!(tokens.len(), 1000);
    }

    #[test]
    fn the_hash_is_lower_case_hex_sha256_of_the_tokens_utf8_bytes() {
        // `echo -n abc | sha256sum`
        assert_eq!(
            TokenHash::of(&SecretString::from("abc")).as_str(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        // Not Base64-decoded first: the hash is of the text, as Java's `token.getBytes(UTF_8)`.
        assert_eq!(
            TokenHash::of(&SecretString::from("é")).as_str(),
            "4a99557e4033c3539de2eb65472017cad5f9557f7a0625a09f1c3f6e2ba69c4c"
        );
    }

    #[test]
    fn the_hash_has_64_hex_characters_and_never_the_token() {
        let token = generate().unwrap();

        let hash = TokenHash::of(&token);

        assert_eq!(hash.as_str().len(), 64);
        assert!(
            hash.as_str()
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        );
        assert!(!hash.as_str().contains(token.expose_secret()));
    }

    #[test]
    fn hashes_are_equal_only_for_equal_tokens() {
        let token = SecretString::from("one");

        assert_eq!(TokenHash::of(&token), TokenHash::of(&SecretString::from("one")));
        assert_ne!(TokenHash::of(&token), TokenHash::of(&SecretString::from("two")));
    }

    #[test]
    fn neither_token_nor_hash_is_printed() {
        let token = generate().unwrap();

        assert!(!format!("{token:?}").contains(token.expose_secret()));
        assert_eq!(format!("{:?}", TokenHash::of(&token)), "TokenHash(***)");
    }
}
