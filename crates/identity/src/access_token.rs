//! Access tokens: ES256 JWTs that Ledgerly signs itself, and the key that signs them.
//!
//! Java counterpart: `identity/internal/application/AccessTokenIssuer.java`, `infrastructure/JwtKeys.java` and
//! the `JwtEncoder`/`JwtDecoder` beans in `infrastructure/IdentityConfig.java` (Nimbus underneath).
//!
//! A token is a compact JWS, ES256 over P-256:
//! - header `{"kid":…,"alg":"ES256"}`;
//! - claims `{"sub":…,"exp":…,"iat":…,"jti":…}`: the user id, ten minutes of life, a random v4 id. No issuer,
//!   no audience, no email and no roles (roles are per team and read from the database on every request).
//!
//! The member order is the one Nimbus writes (its JSON objects are `HashMap`s). Nothing depends on it; it only
//! makes the tokens look alike.
//!
//! Verification follows Nimbus' `DefaultJWTProcessor` with an ES256-only key selector and Spring's
//! `JwtTimestampValidator` (30 s of skew, the application clock): see [`AccessTokens::verify`].

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use base64::Engine;
use base64::alphabet::{STANDARD, URL_SAFE};
use base64::engine::DecodePaddingMode;
use base64::engine::general_purpose::{GeneralPurpose, GeneralPurposeConfig, URL_SAFE_NO_PAD};
use chrono::{DateTime, TimeDelta, Utc};
use ledgerly_shared::clock::Clock;
use ledgerly_shared::id::UserId;
use p256::ecdsa::signature::{Signer, Verifier};
use p256::ecdsa::{Signature, SigningKey, VerifyingKey};
use p256::pkcs8::{DecodePrivateKey, DecodePublicKey};
use secrecy::{ExposeSecret, SecretString};
use serde::Serialize;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::strict_json;

/// Java's `JwtTimestampValidator(Duration.ofSeconds(30))`.
const CLOCK_SKEW: TimeDelta = TimeDelta::seconds(30);

/// Nimbus' `Base64URL.decode`: padding is neither needed nor refused, and the unused bits are not checked.
const LENIENT_BASE64URL: GeneralPurpose = GeneralPurpose::new(
    &URL_SAFE,
    GeneralPurposeConfig::new()
        .with_decode_padding_mode(DecodePaddingMode::Indifferent)
        .with_decode_allow_trailing_bits(true),
);

/// Java's `Base64.getDecoder()`, which `JwtKeys.decode` uses for the configured keys.
const JAVA_BASE64: GeneralPurpose = GeneralPurpose::new(
    &STANDARD,
    GeneralPurposeConfig::new()
        .with_decode_padding_mode(DecodePaddingMode::Indifferent)
        .with_decode_allow_trailing_bits(true),
);

/// Why the configured signing key cannot be used. The application does not start.
#[derive(Debug, thiserror::Error)]
pub enum KeyError {
    /// Java's `IllegalStateException` from `JwtKeys.create`.
    #[error("ledgerly.auth.jwt-private-key and jwt-public-key must be set outside the local and test profiles")]
    Missing,
    /// Java's `IllegalStateException("Invalid JWT key configuration")`. Says nothing about the key itself.
    #[error("Invalid JWT key configuration")]
    Invalid,
    /// The operating system's random number generator failed while making a throw-away key.
    #[error("Could not generate a signing key")]
    NoRandomness,
}

/// The ES256 key pair and the `kid` that names it. Java: `JwtKeys`.
pub struct SigningKeys {
    signing: SigningKey,
    verifying: VerifyingKey,
    kid: String,
}

/// The private key never appears in a `Debug` print.
impl fmt::Debug for SigningKeys {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SigningKeys")
            .field("kid", &self.kid)
            .finish_non_exhaustive()
    }
}

impl SigningKeys {
    /// The configured pair, or a throw-away one when the configuration has none and `generation_allowed` (the
    /// `local` and `test` profiles). Java: `JwtKeys.create`.
    ///
    /// # Errors
    ///
    /// [`KeyError::Missing`] if a key is missing or blank and generation is not allowed, [`KeyError::Invalid`]
    /// if a key cannot be read.
    pub fn from_config(
        private_key: Option<&SecretString>,
        public_key: Option<&str>,
        generation_allowed: bool,
    ) -> Result<Self, KeyError> {
        match (private_key, public_key) {
            (Some(private_key), Some(public_key))
                if !ledgerly_shared::text::is_blank(private_key.expose_secret())
                    && !ledgerly_shared::text::is_blank(public_key) =>
            {
                Self::from_text(private_key, public_key)
            }
            _ if generation_allowed => Self::generate(),
            _ => Err(KeyError::Missing),
        }
    }

    /// A random P-256 key with a random v4 `kid`. Java: `JwtKeys.generate`.
    fn generate() -> Result<Self, KeyError> {
        let mut scalar = Zeroizing::new([0_u8; 32]);
        // A random 256-bit number is a valid scalar unless it is zero or at least the group order, which
        // happens about once in 2^32 tries.
        let signing = loop {
            getrandom::fill(scalar.as_mut_slice()).map_err(|_| KeyError::NoRandomness)?;
            if let Ok(key) = SigningKey::from_slice(scalar.as_slice()) {
                break key;
            }
        };
        let verifying = *signing.verifying_key();
        Ok(Self {
            signing,
            verifying,
            kid: Uuid::new_v4().to_string(),
        })
    }

    /// A PKCS#8 private key and an X.509 `SubjectPublicKeyInfo` public key, each as PEM or bare Base64. The
    /// `kid` is the public key's RFC 7638 thumbprint, so it is the same after every restart. Java:
    /// `JwtKeys.fromPem` with `keyIDFromThumbprint()`.
    ///
    /// Like Java, it does not check that the two keys belong together.
    fn from_text(private_key: &SecretString, public_key: &str) -> Result<Self, KeyError> {
        let private_der = Zeroizing::new(pem_body(private_key.expose_secret()).ok_or(KeyError::Invalid)?);
        let signing = SigningKey::from_pkcs8_der(&private_der).map_err(|_| KeyError::Invalid)?;
        let public_der = pem_body(public_key).ok_or(KeyError::Invalid)?;
        let verifying = VerifyingKey::from_public_key_der(&public_der).map_err(|_| KeyError::Invalid)?;
        let kid = thumbprint(&verifying);
        Ok(Self {
            signing,
            verifying,
            kid,
        })
    }

    /// The `kid` in token headers and in the JWKS.
    #[must_use]
    pub fn kid(&self) -> &str {
        &self.kid
    }

    /// The public half as a JWK. Java: `keys.key().toPublicJWK().toJSONObject()`.
    pub(crate) fn public_jwk(&self) -> PublicJwk {
        let (x, y) = coordinates(&self.verifying);
        PublicJwk {
            kty: "EC",
            crv: "P-256",
            kid: self.kid.clone(),
            x,
            y,
        }
    }
}

/// Java's `JwtKeys.decode`: drop the armour lines (`-----BEGIN …-----`) and all whitespace, then decode
/// Base64.
fn pem_body(text: &str) -> Option<Vec<u8>> {
    let mut rest = text;
    let mut base64 = String::new();
    while let Some(start) = rest.find("-----") {
        base64.push_str(rest.get(..start)?);
        let after = rest.get(start + 5..)?;
        // `-----[A-Z ]+-----`: what does not look like armour stays.
        let label_length = after
            .find(|c: char| !(c.is_ascii_uppercase() || c == ' '))
            .unwrap_or(after.len());
        match after.get(label_length..) {
            Some(tail) if label_length > 0 && tail.starts_with("-----") => rest = tail.get(5..)?,
            _ => {
                base64.push_str("-----");
                rest = after;
            }
        }
    }
    base64.push_str(rest);
    base64.retain(|c| !matches!(c, ' ' | '\t' | '\n' | '\u{b}' | '\u{c}' | '\r'));
    JAVA_BASE64.decode(base64).ok()
}

/// The affine coordinates, Base64url, each padded to the field's 32 bytes as Nimbus' `ECKey` writes them.
fn coordinates(key: &VerifyingKey) -> (String, String) {
    let point = key.to_sec1_point(false);
    let encode = |bytes: Option<&p256::FieldBytes>| bytes.map(|b| URL_SAFE_NO_PAD.encode(b)).unwrap_or_default();
    (encode(point.x()), encode(point.y()))
}

/// RFC 7638: SHA-256 over the required members in lexicographic order, Base64url.
fn thumbprint(key: &VerifyingKey) -> String {
    let (x, y) = coordinates(key);
    let canonical = format!(r#"{{"crv":"P-256","kty":"EC","x":"{x}","y":"{y}"}}"#);
    URL_SAFE_NO_PAD.encode(Sha256::digest(canonical.as_bytes()))
}

/// One entry of `GET /.well-known/jwks.json`, in the member order Java writes. No `d`: the private key is not
/// part of it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct PublicJwk {
    kty: &'static str,
    crv: &'static str,
    kid: String,
    x: String,
    y: String,
}

/// A token that was just signed, and how long it lives.
pub(crate) struct IssuedToken {
    pub(crate) token: SecretString,
    pub(crate) expires_in_seconds: i64,
}

/// Why a token could not be signed: the lifetime does not fit into a timestamp. Only a configuration of
/// centuries gets here.
#[derive(Debug, thiserror::Error)]
#[error("the access token lifetime is out of range")]
pub(crate) struct IssuingFailed;

/// Why a bearer token was not accepted. Deliberately without detail: every reason is the same 401.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidToken;

#[derive(Serialize)]
struct Header<'a> {
    kid: &'a str,
    alg: &'static str,
}

#[derive(Serialize)]
struct Claims {
    sub: String,
    exp: i64,
    iat: i64,
    jti: String,
}

/// Signs and checks access tokens.
#[derive(Clone)]
pub struct AccessTokens {
    keys: Arc<SigningKeys>,
    ttl: TimeDelta,
    clock: Arc<dyn Clock>,
}

impl fmt::Debug for AccessTokens {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AccessTokens")
            .field("keys", &self.keys)
            .field("ttl", &self.ttl)
            .finish_non_exhaustive()
    }
}

impl AccessTokens {
    /// Tokens that live `ttl` (Java's `ledgerly.auth.access-token-ttl`, 10 minutes), timed by `clock`.
    #[must_use]
    pub fn new(keys: Arc<SigningKeys>, ttl: Duration, clock: Arc<dyn Clock>) -> Self {
        Self {
            keys,
            ttl: TimeDelta::from_std(ttl).unwrap_or(TimeDelta::MAX),
            clock,
        }
    }

    /// The signing keys, for the JWKS endpoint.
    pub(crate) fn keys(&self) -> &SigningKeys {
        &self.keys
    }

    /// A token for `user`. Java: `AccessTokenIssuer.issue` and `ttlSeconds`.
    pub(crate) fn issue(&self, user: UserId) -> Result<IssuedToken, IssuingFailed> {
        let now = self.clock.now();
        let expires = now.checked_add_signed(self.ttl).ok_or(IssuingFailed)?;
        let header = Header {
            kid: &self.keys.kid,
            alg: "ES256",
        };
        let claims = Claims {
            sub: user.to_string(),
            // `Date` keeps milliseconds and Nimbus writes whole seconds, rounding down.
            exp: expires.timestamp(),
            iat: now.timestamp(),
            jti: Uuid::new_v4().to_string(),
        };
        let signing_input = format!("{}.{}", segment(&header)?, segment(&claims)?);
        let signature: Signature = self.keys.signing.sign(signing_input.as_bytes());
        Ok(IssuedToken {
            token: SecretString::from(format!(
                "{signing_input}.{}",
                URL_SAFE_NO_PAD.encode(signature.to_bytes())
            )),
            expires_in_seconds: self.ttl.num_seconds(),
        })
    }

    /// The user a token was issued to, if it is one of ours and still valid.
    ///
    /// What Nimbus and Spring check, in effect:
    /// - three Base64url segments; the header and the claims are JSON objects without duplicate members;
    /// - `alg` is exactly `ES256` (so not `none` and not `HS256` with the public key as the secret); a `kid`, if
    ///   present, is ours; `typ`, if present, is `JWT` or `JOSE`; no `crit` and no `x5t#S256` (Nimbus would
    ///   find no key for either);
    /// - the signature is 64 bytes and verifies with our public key;
    /// - the registered claims have their JSON types (`exp`, `iat`, `nbf` numbers; `sub`, `jti`, `iss`
    ///   strings; `aud` a string or strings);
    /// - `exp`, if present, is not more than 30 s in the past; `nbf`, if present, not more than 30 s in the
    ///   future;
    /// - `sub` is a user id.
    ///
    /// # Errors
    ///
    /// [`InvalidToken`] for any failed check; the reason is not told.
    pub fn verify(&self, token: &str) -> Result<UserId, InvalidToken> {
        let mut segments = token.split('.');
        let (Some(header), Some(payload), Some(signature), None) =
            (segments.next(), segments.next(), segments.next(), segments.next())
        else {
            return Err(InvalidToken);
        };
        let header_json = object(header)?;
        self.check_header(&header_json)?;
        let claims = object(payload)?;
        let signature = LENIENT_BASE64URL.decode(signature).map_err(|_| InvalidToken)?;
        let signature = Signature::from_slice(&signature).map_err(|_| InvalidToken)?;
        let signing_input = token.get(..header.len() + 1 + payload.len()).ok_or(InvalidToken)?;
        self.keys
            .verifying
            .verify(signing_input.as_bytes(), &signature)
            .map_err(|_| InvalidToken)?;
        check_claim_types(&claims)?;
        self.check_time(&claims)?;
        match claims.get("sub") {
            Some(Value::String(subject)) => subject.parse().map_err(|_| InvalidToken),
            _ => Err(InvalidToken),
        }
    }

    fn check_header(&self, header: &Map<String, Value>) -> Result<(), InvalidToken> {
        if header.get("alg") != Some(&Value::from("ES256")) {
            return Err(InvalidToken);
        }
        match header.get("kid") {
            None => {}
            Some(Value::String(kid)) if *kid == self.keys.kid => {}
            Some(_) => return Err(InvalidToken),
        }
        match header.get("typ") {
            None => {}
            Some(Value::String(typ)) if typ.eq_ignore_ascii_case("JWT") || typ.eq_ignore_ascii_case("JOSE") => {}
            Some(_) => return Err(InvalidToken),
        }
        if header.contains_key("crit") || header.contains_key("x5t#S256") {
            return Err(InvalidToken);
        }
        Ok(())
    }

    /// Spring's `JwtTimestampValidator` with the application clock.
    fn check_time(&self, claims: &Map<String, Value>) -> Result<(), InvalidToken> {
        let now = self.clock.now();
        if let Some(expires) = instant(claims.get("exp"))
            && now - CLOCK_SKEW > expires
        {
            return Err(InvalidToken);
        }
        if let Some(not_before) = instant(claims.get("nbf"))
            && now + CLOCK_SKEW < not_before
        {
            return Err(InvalidToken);
        }
        Ok(())
    }
}

fn segment(value: &impl Serialize) -> Result<String, IssuingFailed> {
    let json = serde_json::to_vec(value).map_err(|_| IssuingFailed)?;
    Ok(URL_SAFE_NO_PAD.encode(json))
}

/// A Base64url segment that holds a JSON object. Nimbus parses with Gson, which refuses duplicate members.
fn object(segment: &str) -> Result<Map<String, Value>, InvalidToken> {
    let bytes = LENIENT_BASE64URL.decode(segment).map_err(|_| InvalidToken)?;
    match strict_json::parse(&bytes) {
        Some(Value::Object(object)) => Ok(object),
        _ => Err(InvalidToken),
    }
}

/// Nimbus' `JWTClaimsSet.parse`: a registered claim of the wrong type makes the token unreadable.
fn check_claim_types(claims: &Map<String, Value>) -> Result<(), InvalidToken> {
    for (name, value) in claims {
        let fits = match name.as_str() {
            "iss" | "sub" | "jti" => value.is_string(),
            "aud" => value.is_string() || value.as_array().is_some_and(|all| all.iter().all(Value::is_string)),
            "exp" | "iat" | "nbf" => value.is_number(),
            _ => true,
        };
        if !fits {
            return Err(InvalidToken);
        }
    }
    Ok(())
}

/// A NumericDate as Nimbus reads it: `number.longValue()` seconds.
fn instant(value: Option<&Value>) -> Option<DateTime<Utc>> {
    let value = value?;
    #[expect(
        clippy::cast_possible_truncation,
        reason = "Java's Number.longValue() truncates and saturates too"
    )]
    let seconds = value
        .as_i64()
        .or_else(|| value.as_f64().map(|seconds| seconds as i64))?;
    // An instant too far out for chrono is beyond any clock, as it is for Java's.
    Some(DateTime::from_timestamp(seconds, 0).unwrap_or(if seconds < 0 {
        DateTime::<Utc>::MIN_UTC
    } else {
        DateTime::<Utc>::MAX_UTC
    }))
}

#[cfg(test)]
mod tests;
