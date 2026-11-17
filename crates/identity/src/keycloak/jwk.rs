//! The identity provider's public keys and the signatures they check.
//!
//! Java counterpart: Nimbus' `JWSVerificationKeySelector` and its verifiers (`RSASSAVerifier`, `ECDSAVerifier`),
//! as Spring's `NimbusJwtDecoder` sets them up from the JWK set, and
//! `JwtDecoderProviderConfigurationUtils.getSignatureAlgorithms`, which decides the accepted algorithms from the
//! keys once, when the decoder is built.
//!
//! What a key must be to be used (Spring's `JWKMatcher`): RSA or EC, public only (no `d`), and `use` absent or
//! `sig`. Keycloak also publishes an `RSA-OAEP` key for encryption; it is never used here.
//!
//! The algorithms are those Nimbus accepts and `ring` implements: RS256/384/512, PS256/384/512, ES256 and
//! ES384. Nimbus also knows ES256K and ES512; a token signed so is refused here (open question in the M11
//! learning log; Keycloak signs RS256 unless a realm is configured otherwise).

use ring::signature::{self, RsaPublicKeyComponents, UnparsedPublicKey, VerificationAlgorithm};
use serde_json::{Map, Value};

use crate::access_token::LENIENT_BASE64URL;

use base64::Engine;

/// A JWS algorithm this application can check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Algorithm {
    Rs256,
    Rs384,
    Rs512,
    Ps256,
    Ps384,
    Ps512,
    Es256,
    Es384,
}

impl Algorithm {
    const RSA_FAMILY: [Self; 6] = [
        Self::Rs256,
        Self::Rs384,
        Self::Rs512,
        Self::Ps256,
        Self::Ps384,
        Self::Ps512,
    ];
    const EC_FAMILY: [Self; 2] = [Self::Es256, Self::Es384];

    /// The `alg` of a JWS header or a JWK.
    pub(crate) fn named(name: &str) -> Option<Self> {
        Some(match name {
            "RS256" => Self::Rs256,
            "RS384" => Self::Rs384,
            "RS512" => Self::Rs512,
            "PS256" => Self::Ps256,
            "PS384" => Self::Ps384,
            "PS512" => Self::Ps512,
            "ES256" => Self::Es256,
            "ES384" => Self::Es384,
            _ => return None,
        })
    }

    const fn is_rsa(self) -> bool {
        !matches!(self, Self::Es256 | Self::Es384)
    }
}

/// A public key of the JWK set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Jwk {
    kid: Option<String>,
    /// The key's own `alg`, if it names one.
    alg: Option<String>,
    /// `x5t#S256`, which a token's header may name instead of (or besides) a `kid`.
    thumbprint: Option<String>,
    material: Material,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Material {
    Rsa {
        n: Vec<u8>,
        e: Vec<u8>,
    },
    /// The uncompressed point `04 || x || y`.
    Ec {
        curve: Curve,
        point: Vec<u8>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Curve {
    P256,
    P384,
    /// P-521, secp256k1: known to Nimbus, checked by nothing here.
    Other,
}

/// The usable keys of a JWK set document (`{"keys": [...]}`), or `None` if it is not one. Keys that cannot be
/// read are left out, as Nimbus leaves out keys its matcher does not select.
pub(crate) fn parse_set(document: &Value) -> Option<Vec<Jwk>> {
    let keys = document.get("keys")?.as_array()?;
    Some(keys.iter().filter_map(Value::as_object).filter_map(parse_key).collect())
}

fn parse_key(key: &Map<String, Value>) -> Option<Jwk> {
    let text = |name: &str| key.get(name).and_then(Value::as_str);
    let bytes = |name: &str| text(name).and_then(|value| LENIENT_BASE64URL.decode(value).ok());
    if key.contains_key("d") || text("use").is_some_and(|usage| usage != "sig") {
        return None;
    }
    let material = match text("kty")? {
        "RSA" => Material::Rsa {
            n: bytes("n")?,
            e: bytes("e")?,
        },
        "EC" => {
            let (curve, size) = match text("crv")? {
                "P-256" => (Curve::P256, 32),
                "P-384" => (Curve::P384, 48),
                _ => (Curve::Other, 0),
            };
            let (x, y) = (bytes("x")?, bytes("y")?);
            if curve != Curve::Other && (x.len() != size || y.len() != size) {
                return None;
            }
            let mut point = Vec::with_capacity(1 + x.len() + y.len());
            point.push(0x04);
            point.extend_from_slice(&x);
            point.extend_from_slice(&y);
            Material::Ec { curve, point }
        }
        _ => return None,
    };
    Some(Jwk {
        kid: text("kid").map(str::to_owned),
        alg: text("alg").map(str::to_owned),
        thumbprint: text("x5t#S256").map(str::to_owned),
        material,
    })
}

/// The algorithms a token may be signed with, decided from the keys: a key's own `alg`, or its whole family
/// when it names none. Java: `JwtDecoderProviderConfigurationUtils.getSignatureAlgorithms`.
pub(crate) fn algorithms(keys: &[Jwk]) -> Vec<Algorithm> {
    let mut found = Vec::new();
    for key in keys {
        let family: Vec<Algorithm> = match (&key.alg, &key.material) {
            (Some(alg), _) => Algorithm::named(alg).into_iter().collect(),
            (None, Material::Rsa { .. }) => Algorithm::RSA_FAMILY.to_vec(),
            (None, Material::Ec { .. }) => Algorithm::EC_FAMILY.to_vec(),
        };
        for algorithm in family {
            if !found.contains(&algorithm) {
                found.push(algorithm);
            }
        }
    }
    found
}

/// What the token's header asks for.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Wanted<'a> {
    pub algorithm: Algorithm,
    pub kid: Option<&'a str>,
    pub thumbprint: Option<&'a str>,
}

impl Jwk {
    /// Nimbus' `JWKMatcher.forJWSHeader`: the key type and curve of the algorithm, the same `kid` and
    /// `x5t#S256` when the header names them, and the same `alg` when the key names one.
    pub(crate) fn matches(&self, wanted: Wanted<'_>) -> bool {
        let fits = match (&self.material, wanted.algorithm) {
            (Material::Rsa { .. }, algorithm) => algorithm.is_rsa(),
            (Material::Ec { curve, .. }, Algorithm::Es256) => *curve == Curve::P256,
            (Material::Ec { curve, .. }, Algorithm::Es384) => *curve == Curve::P384,
            (Material::Ec { .. }, _) => false,
        };
        fits && wanted.kid.is_none_or(|kid| self.kid.as_deref() == Some(kid))
            && wanted
                .thumbprint
                .is_none_or(|thumbprint| self.thumbprint.as_deref() == Some(thumbprint))
            && self
                .alg
                .as_deref()
                .is_none_or(|alg| Algorithm::named(alg) == Some(wanted.algorithm))
    }

    /// Whether `signature` signs `message` with this key under `algorithm`. RSA keys under 2048 bits are
    /// refused, as Nimbus refuses them.
    pub(crate) fn verifies(&self, algorithm: Algorithm, message: &[u8], signature: &[u8]) -> bool {
        match &self.material {
            Material::Rsa { n, e } => {
                let parameters: &signature::RsaParameters = match algorithm {
                    Algorithm::Rs256 => &signature::RSA_PKCS1_2048_8192_SHA256,
                    Algorithm::Rs384 => &signature::RSA_PKCS1_2048_8192_SHA384,
                    Algorithm::Rs512 => &signature::RSA_PKCS1_2048_8192_SHA512,
                    Algorithm::Ps256 => &signature::RSA_PSS_2048_8192_SHA256,
                    Algorithm::Ps384 => &signature::RSA_PSS_2048_8192_SHA384,
                    Algorithm::Ps512 => &signature::RSA_PSS_2048_8192_SHA512,
                    Algorithm::Es256 | Algorithm::Es384 => return false,
                };
                RsaPublicKeyComponents { n, e }
                    .verify(parameters, message, signature)
                    .is_ok()
            }
            Material::Ec { point, .. } => {
                let verification: &dyn VerificationAlgorithm = match algorithm {
                    Algorithm::Es256 => &signature::ECDSA_P256_SHA256_FIXED,
                    Algorithm::Es384 => &signature::ECDSA_P384_SHA384_FIXED,
                    _ => return false,
                };
                UnparsedPublicKey::new(verification, point)
                    .verify(message, signature)
                    .is_ok()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn keycloak_set() -> Value {
        // The shape of Keycloak's JWK set: a signing key with `alg`, and an encryption key.
        json!({"keys": [
            {"kid": "sig-1", "kty": "RSA", "alg": "RS256", "use": "sig", "n": "AQAB", "e": "AQAB",
             "x5t#S256": "abc"},
            {"kid": "enc-1", "kty": "RSA", "alg": "RSA-OAEP", "use": "enc", "n": "AQAB", "e": "AQAB"}
        ]})
    }

    #[test]
    fn only_signing_keys_are_used_and_they_decide_the_algorithms() {
        let keys = parse_set(&keycloak_set()).unwrap();

        assert_eq!(keys.len(), 1);
        assert_eq!(algorithms(&keys), [Algorithm::Rs256]);
    }

    #[test]
    fn a_key_without_alg_allows_its_whole_family() {
        let keys = parse_set(&json!({"keys": [
            {"kty": "RSA", "n": "AQAB", "e": "AQAB"},
            {"kty": "EC", "crv": "P-256", "x": "A".repeat(43), "y": "A".repeat(43)}
        ]}))
        .unwrap();

        assert_eq!(algorithms(&keys).len(), 8);
    }

    #[test]
    fn private_keys_unknown_types_and_broken_keys_are_left_out() {
        let keys = parse_set(&json!({"keys": [
            {"kty": "RSA", "n": "AQAB", "e": "AQAB", "d": "AQAB"},
            {"kty": "oct", "k": "AQAB"},
            {"kty": "RSA", "n": "AQAB"},
            {"kty": "EC", "crv": "P-256", "x": "AQAB", "y": "AQAB"},
            "not an object"
        ]}))
        .unwrap();

        assert_eq!(keys, []);
        assert_eq!(parse_set(&json!({"no": "keys"})), None);
    }

    #[test]
    fn a_key_is_chosen_by_type_kid_thumbprint_and_alg() {
        let keys = parse_set(&keycloak_set()).unwrap();
        let key = &keys[0];
        let wanted = |algorithm, kid, thumbprint| Wanted {
            algorithm,
            kid,
            thumbprint,
        };

        assert!(key.matches(wanted(Algorithm::Rs256, Some("sig-1"), None)));
        assert!(key.matches(wanted(Algorithm::Rs256, None, None)));
        assert!(key.matches(wanted(Algorithm::Rs256, None, Some("abc"))));
        assert!(!key.matches(wanted(Algorithm::Rs256, Some("other"), None)));
        assert!(!key.matches(wanted(Algorithm::Rs256, None, Some("other"))));
        assert!(
            !key.matches(wanted(Algorithm::Rs384, Some("sig-1"), None)),
            "the key names RS256"
        );
        assert!(!key.matches(wanted(Algorithm::Es256, Some("sig-1"), None)));
    }

    #[test]
    fn an_ec_signature_is_checked_with_the_key_of_the_set() {
        let rng = ring::rand::SystemRandom::new();
        let pkcs8 = signature::EcdsaKeyPair::generate_pkcs8(&signature::ECDSA_P256_SHA256_FIXED_SIGNING, &rng).unwrap();
        let pair =
            signature::EcdsaKeyPair::from_pkcs8(&signature::ECDSA_P256_SHA256_FIXED_SIGNING, pkcs8.as_ref(), &rng)
                .unwrap();
        let public = ring::signature::KeyPair::public_key(&pair).as_ref();
        let encode = |bytes: &[u8]| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes);
        let keys = parse_set(&json!({"keys": [
            {"kty": "EC", "crv": "P-256", "x": encode(&public[1..33]), "y": encode(&public[33..])}
        ]}))
        .unwrap();
        let signature = pair.sign(&rng, b"header.claims").unwrap();

        assert!(keys[0].verifies(Algorithm::Es256, b"header.claims", signature.as_ref()));
        assert!(!keys[0].verifies(Algorithm::Es256, b"header.claimz", signature.as_ref()));
        assert!(!keys[0].verifies(Algorithm::Es384, b"header.claims", signature.as_ref()));
        assert!(!keys[0].verifies(Algorithm::Rs256, b"header.claims", signature.as_ref()));
    }

    #[test]
    fn an_rsa_key_too_short_for_nimbus_verifies_nothing() {
        let keys = parse_set(&keycloak_set()).unwrap();

        assert!(!keys[0].verifies(Algorithm::Rs256, b"message", &[0; 256]));
    }
}
