//! Password hashes that Spring Security reads and writes.
//!
//! Java counterpart: `Argon2PasswordEncoder.defaultsForSpringSecurity_v5_8()` (Bouncy Castle underneath), wired
//! in `identity/internal/infrastructure/IdentityConfig.java`. A hash written by either application verifies in
//! the other (ADR 0005), so the stored text is exactly Spring's `Argon2EncodingUtils` format:
//!
//! ```text
//! $argon2id$v=19$m=16384,t=2,p=1$<salt>$<hash>
//! ```
//!
//! with salt (16 bytes) and hash (32 bytes) in standard Base64 without padding.
//!
//! Hashing is deliberately slow (16 MiB, two passes, tens of milliseconds). The functions here block; the use
//! cases call them on Tokio's blocking pool.

use std::sync::LazyLock;

use argon2::{Algorithm, Argon2, Params, Version};
use base64::Engine;
use base64::alphabet::STANDARD;
use base64::engine::DecodePaddingMode;
use base64::engine::general_purpose::{GeneralPurpose, GeneralPurposeConfig, STANDARD_NO_PAD};
use secrecy::{ExposeSecret, SecretString};
use subtle::ConstantTimeEq;
use zeroize::Zeroizing;

/// Spring's `DEFAULT_MEMORY`: 16 MiB, in KiB.
const MEMORY_KIB: u32 = 1 << 14;
/// Spring's `DEFAULT_ITERATIONS`.
const ITERATIONS: u32 = 2;
/// Spring's `DEFAULT_PARALLELISM`.
const PARALLELISM: u32 = 1;
/// Spring's `DEFAULT_SALT_LENGTH`.
const SALT_LENGTH: usize = 16;
/// Spring's `DEFAULT_HASH_LENGTH`.
const HASH_LENGTH: usize = 32;

/// What `Login` verifies an unknown email against, so that it takes as long as a wrong password. Java:
/// `encoder.encode("not-a-real-password")` in `Login`'s constructor.
const DUMMY_PASSWORD: &str = "not-a-real-password";

/// Java's `Base64.getDecoder()`: the standard alphabet, padding optional, and the unused bits of the last
/// character not checked.
const JAVA_BASE64: GeneralPurpose = GeneralPurpose::new(
    &STANDARD,
    GeneralPurposeConfig::new()
        .with_decode_padding_mode(DecodePaddingMode::Indifferent)
        .with_decode_allow_trailing_bits(true),
);

/// Why a password could not be hashed. Only a broken random number generator or a bug gets here.
#[derive(Debug, thiserror::Error)]
#[error("the password could not be hashed")]
pub(crate) struct HashingFailed;

/// A stored password hash, as the `users.password_hash` column holds it.
///
/// Not secret the way a password is, but never logged either (`LogHygieneTest` looks for `argon2id`).
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct PasswordHash(String);

impl PasswordHash {
    /// A hash read from the database. It is checked only when a password is verified against it.
    pub(crate) const fn stored(text: String) -> Self {
        Self(text)
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for PasswordHash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PasswordHash(***)")
    }
}

/// Hashes `password` with a fresh random salt. Java: `Argon2PasswordEncoder.encode`.
///
/// Java hashes the UTF-8 bytes of the password (Bouncy Castle's default `PasswordConverter.UTF8`), and so does
/// this.
pub(crate) fn hash(password: &SecretString) -> Result<PasswordHash, HashingFailed> {
    let mut salt = [0_u8; SALT_LENGTH];
    getrandom::fill(&mut salt).map_err(|_| HashingFailed)?;
    hash_with_salt(password, &salt)
}

fn hash_with_salt(password: &SecretString, salt: &[u8; SALT_LENGTH]) -> Result<PasswordHash, HashingFailed> {
    let params = Params::new(MEMORY_KIB, ITERATIONS, PARALLELISM, Some(HASH_LENGTH)).map_err(|_| HashingFailed)?;
    let mut output = Zeroizing::new([0_u8; HASH_LENGTH]);
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
        .hash_password_into(password.expose_secret().as_bytes(), salt, output.as_mut_slice())
        .map_err(|_| HashingFailed)?;
    Ok(PasswordHash(format!(
        "$argon2id$v=19$m={MEMORY_KIB},t={ITERATIONS},p={PARALLELISM}${}${}",
        STANDARD_NO_PAD.encode(salt),
        STANDARD_NO_PAD.encode(output.as_slice())
    )))
}

/// Whether `password` is the one `stored` was made from. Java: `Argon2PasswordEncoder.matches`.
///
/// The parameters come from the stored text, as in Java, so hashes made with other settings still verify. A
/// stored text Spring cannot read is not a match; Java logs it as `Malformed password hash`, and so does this,
/// without the text. The comparison takes the same time wherever the bytes differ.
pub(crate) fn matches(password: &SecretString, stored: &PasswordHash) -> bool {
    let Some(decoded) = Decoded::parse(stored.as_str()) else {
        tracing::warn!("Malformed password hash");
        return false;
    };
    let Ok(params) = Params::new(
        decoded.memory,
        decoded.iterations,
        decoded.parallelism,
        Some(decoded.hash.len()),
    ) else {
        tracing::warn!("Malformed password hash");
        return false;
    };
    let mut computed = Zeroizing::new(vec![0_u8; decoded.hash.len()]);
    let hashed = Argon2::new(decoded.algorithm, decoded.version, params).hash_password_into(
        password.expose_secret().as_bytes(),
        &decoded.salt,
        computed.as_mut_slice(),
    );
    hashed.is_ok() && bool::from(computed.as_slice().ct_eq(&decoded.hash))
}

/// Spends the time of verifying a password against a real hash, and always fails. Java: `Login` matching an
/// unknown email's password against its dummy hash.
pub(crate) fn match_nothing(password: &SecretString) -> bool {
    static DUMMY: LazyLock<Option<PasswordHash>> = LazyLock::new(|| hash(&SecretString::from(DUMMY_PASSWORD)).ok());
    if let Some(dummy) = DUMMY.as_ref() {
        let _ = matches(password, dummy);
    }
    false
}

/// A PHC string as Spring's `Argon2EncodingUtils.decode` reads it.
struct Decoded {
    algorithm: Algorithm,
    version: Version,
    memory: u32,
    iterations: u32,
    parallelism: u32,
    salt: Vec<u8>,
    hash: Vec<u8>,
}

impl Decoded {
    /// `$type[$v=N]$m=M,t=T,p=P$salt$hash`. Java splits at `$` and drops trailing empty parts; whatever comes
    /// after the hash is ignored, as there.
    fn parse(text: &str) -> Option<Self> {
        let mut parts: Vec<&str> = text.split('$').collect();
        while parts.last().is_some_and(|part| part.is_empty()) {
            parts.pop();
        }
        if parts.len() < 4 {
            return None;
        }
        let mut parts = parts.into_iter().skip(1).peekable();
        let algorithm = match parts.next()? {
            "argon2d" => Algorithm::Argon2d,
            "argon2i" => Algorithm::Argon2i,
            "argon2id" => Algorithm::Argon2id,
            _ => return None,
        };
        // Bouncy Castle's default when the text names no version.
        let mut version = Version::V0x10;
        if let Some(number) = parts.next_if(|part| part.starts_with("v=")) {
            version = match java_int(number.get(2..)?)? {
                0x10 => Version::V0x10,
                0x13 => Version::V0x13,
                _ => return None,
            };
        }
        let performance: Vec<&str> = parts.next()?.split(',').collect();
        let [memory, iterations, parallelism] = performance.as_slice() else {
            return None;
        };
        let salt = JAVA_BASE64.decode(parts.next()?).ok()?;
        let hash = JAVA_BASE64.decode(parts.next()?).ok()?;
        Some(Self {
            algorithm,
            version,
            memory: positive(java_int(memory.strip_prefix("m=")?)?)?,
            iterations: positive(java_int(iterations.strip_prefix("t=")?)?)?,
            parallelism: positive(java_int(parallelism.strip_prefix("p=")?)?)?,
            salt,
            hash,
        })
    }
}

/// `Integer.parseInt`: an optional sign and ASCII digits, within `int`.
fn java_int(text: &str) -> Option<i32> {
    let digits = text.strip_prefix(['+', '-']).unwrap_or(text);
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

fn positive(value: i32) -> Option<u32> {
    u32::try_from(value).ok().filter(|&value| value > 0)
}

#[cfg(test)]
mod tests {
    use base64::engine::general_purpose::STANDARD as PADDED;

    use super::*;

    fn secret(text: &str) -> SecretString {
        SecretString::from(text)
    }

    /// What Spring's `Argon2EncodingUtils.encode` writes, built here from the documented encoding with the
    /// `argon2` crate directly, so that the test does not trust the code it tests.
    fn spring_hash(password: &str, salt: &[u8], algorithm: (&str, Algorithm), version: (u32, Version)) -> String {
        let params = Params::new(MEMORY_KIB, ITERATIONS, PARALLELISM, Some(HASH_LENGTH)).unwrap();
        let mut output = [0_u8; HASH_LENGTH];
        Argon2::new(algorithm.1, version.1, params)
            .hash_password_into(password.as_bytes(), salt, &mut output)
            .unwrap();
        format!(
            "${}$v={}$m=16384,t=2,p=1${}${}",
            algorithm.0,
            version.0,
            STANDARD_NO_PAD.encode(salt),
            STANDARD_NO_PAD.encode(output)
        )
    }

    #[test]
    fn a_hash_is_springs_phc_string_with_springs_parameters() {
        let hashed = hash(&secret("correct-horse-battery")).unwrap();

        let parts: Vec<&str> = hashed.as_str().split('$').collect();
        assert_eq!(parts[..4], ["", "argon2id", "v=19", "m=16384,t=2,p=1"]);
        assert_eq!(parts.len(), 6);
        // 16 bytes are 22 characters of unpadded Base64, 32 bytes are 43.
        assert_eq!((parts[4].len(), parts[5].len()), (22, 43));
        for encoded in &parts[4..] {
            assert!(
                encoded
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'+' || b == b'/'),
                "{encoded}"
            );
        }
        assert!(!hashed.as_str().contains("correct-horse-battery"));
    }

    #[test]
    fn the_hash_is_exactly_what_springs_encoding_gives_for_the_same_salt() {
        let salt = *b"0123456789abcdef";

        let ours = hash_with_salt(&secret("S3cret-Passw0rd-xyz"), &salt).unwrap();

        assert_eq!(
            ours.as_str(),
            spring_hash(
                "S3cret-Passw0rd-xyz",
                &salt,
                ("argon2id", Algorithm::Argon2id),
                (19, Version::V0x13)
            )
        );
    }

    #[test]
    fn every_hash_has_its_own_salt() {
        let password = secret("correct-horse-battery");

        assert_ne!(hash(&password).unwrap(), hash(&password).unwrap());
    }

    #[test]
    fn the_right_password_matches_and_any_other_does_not() {
        let stored = hash(&secret("correct-horse-battery")).unwrap();

        assert!(matches(&secret("correct-horse-battery"), &stored));
        assert!(!matches(&secret("correct-horse-batterY"), &stored));
        assert!(!matches(&secret(""), &stored));
    }

    #[test]
    fn a_hash_built_to_springs_documented_encoding_verifies() {
        let stored = spring_hash(
            "S3cret-Passw0rd-xyz",
            &[7; 16],
            ("argon2id", Algorithm::Argon2id),
            (19, Version::V0x13),
        );

        assert!(matches(&secret("S3cret-Passw0rd-xyz"), &PasswordHash::stored(stored)));
    }

    #[test]
    fn passwords_are_hashed_as_utf8_like_bouncy_castles_default() {
        let stored = spring_hash(
            "Pässwört-ünïcödé-€",
            &[1; 16],
            ("argon2id", Algorithm::Argon2id),
            (19, Version::V0x13),
        );

        assert!(matches(&secret("Pässwört-ünïcödé-€"), &PasswordHash::stored(stored)));
    }

    #[test]
    fn hashes_with_other_types_versions_and_padding_verify_as_in_spring() {
        let argon2i = spring_hash(
            "pw-123456789",
            &[2; 16],
            ("argon2i", Algorithm::Argon2i),
            (19, Version::V0x13),
        );
        let version_16 = spring_hash(
            "pw-123456789",
            &[3; 16],
            ("argon2id", Algorithm::Argon2id),
            (16, Version::V0x10),
        );
        // Java's Base64 decoder takes padding too.
        let padded = {
            let mut output = [0_u8; HASH_LENGTH];
            Argon2::new(
                Algorithm::Argon2id,
                Version::V0x13,
                Params::new(MEMORY_KIB, ITERATIONS, PARALLELISM, Some(HASH_LENGTH)).unwrap(),
            )
            .hash_password_into(b"pw-123456789", &[4; 16], &mut output)
            .unwrap();
            format!(
                "$argon2id$v=19$m=16384,t=2,p=1${}${}",
                PADDED.encode([4; 16]),
                PADDED.encode(output)
            )
        };
        // Without a version, Bouncy Castle's default 0x10 applies.
        let no_version = version_16.replace("$v=16", "");

        for stored in [argon2i, version_16, padded, no_version] {
            assert!(
                matches(&secret("pw-123456789"), &PasswordHash::stored(stored.clone())),
                "{stored}"
            );
        }
    }

    #[test]
    fn a_malformed_stored_hash_matches_nothing() {
        let good = spring_hash(
            "pw-123456789",
            &[5; 16],
            ("argon2id", Algorithm::Argon2id),
            (19, Version::V0x13),
        );
        for stored in [
            String::new(),
            "plain-text".to_owned(),
            "$argon2id$v=19$m=16384,t=2,p=1".to_owned(),
            good.replace("argon2id", "argon2x"),
            good.replace("v=19", "v=18"),
            good.replace("m=16384,t=2,p=1", "m=16384,t=2"),
            good.replace("m=16384", "x=16384"),
            good.replace("t=2", "t=two"),
            good.replace("p=1", "p=0"),
            format!("{good}%"),
        ] {
            assert!(
                !matches(&secret("pw-123456789"), &PasswordHash::stored(stored.clone())),
                "{stored}"
            );
        }
    }

    #[test]
    fn matching_nothing_never_matches() {
        assert!(!match_nothing(&secret(DUMMY_PASSWORD)));
        assert!(!match_nothing(&secret("anything")));
    }

    #[test]
    fn neither_debug_shows_the_hash() {
        let stored = hash(&secret("correct-horse-battery")).unwrap();

        assert_eq!(format!("{stored:?}"), "PasswordHash(***)");
    }
}
