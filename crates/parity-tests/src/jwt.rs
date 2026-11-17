//! Reading access tokens and forging bad ones, without the code under test.
//!
//! Java's `AuthApiTest` uses Nimbus for this. The suite must not depend on the workspace's own token code
//! (ADR 0011), so the few things it needs are here.

use p256::ecdsa::signature::Signer;
use p256::ecdsa::{Signature, SigningKey};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::encoding::{base64url_decode, base64url_unpadded, hmac_sha256};

/// The header and the claims of a JWT, decoded but not verified.
///
/// # Panics
///
/// Panics if `token` is not three Base64url segments of which the first two are JSON.
#[must_use]
pub fn decode(token: &str) -> (Value, Value) {
    let segments: Vec<&str> = token.split('.').collect();
    assert_eq!(segments.len(), 3, "a JWT has three segments: {token}");
    let part = |segment: &str| -> Value {
        let bytes = base64url_decode(segment).unwrap_or_else(|| panic!("not Base64url: {segment}"));
        serde_json::from_slice(&bytes).unwrap_or_else(|error| panic!("not JSON ({error}): {segment}"))
    };
    (part(segments[0]), part(segments[1]))
}

fn segment(value: &Value) -> String {
    base64url_unpadded(value.to_string().as_bytes())
}

/// The claims Ledgerly puts into a token, for a subject of the test's choosing, valid for ten minutes from now.
#[must_use]
pub fn claims_for(subject: Uuid) -> Value {
    let now = chrono::Utc::now().timestamp();
    json!({"sub": subject, "iat": now, "exp": now + 600, "jti": Uuid::new_v4()})
}

/// `token` with another subject in its payload and the original signature.
#[must_use]
pub fn with_another_subject(token: &str) -> String {
    let segments: Vec<&str> = token.split('.').collect();
    let (_, mut claims) = decode(token);
    claims["sub"] = json!(Uuid::new_v4());
    format!("{}.{}.{}", segments[0], segment(&claims), segments[2])
}

/// An unsigned token: `alg` is `none` and the signature is empty.
#[must_use]
pub fn unsigned(claims: &Value) -> String {
    format!("{}.{}.", segment(&json!({"alg": "none"})), segment(claims))
}

/// A token signed with HS256 and `secret`: the algorithm-confusion attack uses public key material as the
/// secret.
#[must_use]
pub fn hs256(claims: &Value, secret: &[u8]) -> String {
    let signing_input = format!("{}.{}", segment(&json!({"alg": "HS256"})), segment(claims));
    let signature = hmac_sha256(secret, signing_input.as_bytes());
    format!("{signing_input}.{}", base64url_unpadded(&signature))
}

/// A correctly formed ES256 token, signed with a P-256 key that is not the server's.
#[must_use]
pub fn es256_with_an_attackers_key(claims: &Value) -> String {
    // Any valid scalar will do. It is fixed so that the suite needs no random number generator.
    let key = SigningKey::from_slice(&[0x42; 32]).expect("a 32-byte scalar below the group order is a key");
    let signing_input = format!(
        "{}.{}",
        segment(&json!({"alg": "ES256", "kid": "attacker"})),
        segment(claims)
    );
    let signature: Signature = key.sign(signing_input.as_bytes());
    format!("{signing_input}.{}", base64url_unpadded(&signature.to_bytes()))
}

#[cfg(test)]
mod tests {
    use p256::ecdsa::VerifyingKey;
    use p256::ecdsa::signature::Verifier;

    use super::*;

    #[test]
    fn a_token_round_trips_through_decode() {
        let subject = Uuid::new_v4();
        let token = unsigned(&claims_for(subject));

        let (header, claims) = decode(&token);

        assert_eq!(header, json!({"alg": "none"}));
        assert_eq!(claims["sub"], subject.to_string());
        assert_eq!(claims["exp"].as_i64().unwrap() - claims["iat"].as_i64().unwrap(), 600);
        assert!(token.ends_with('.'));
    }

    #[test]
    fn another_subject_changes_only_the_payload() {
        let original = es256_with_an_attackers_key(&claims_for(Uuid::new_v4()));

        let tampered = with_another_subject(&original);

        let (before, after): (Vec<&str>, Vec<&str>) = (original.split('.').collect(), tampered.split('.').collect());
        assert_eq!((before[0], before[2]), (after[0], after[2]));
        assert_ne!(decode(&original).1["sub"], decode(&tampered).1["sub"]);
        assert_eq!(decode(&original).1["jti"], decode(&tampered).1["jti"]);
    }

    #[test]
    fn the_attackers_token_is_a_valid_es256_token_for_the_attackers_key() {
        let token = es256_with_an_attackers_key(&claims_for(Uuid::new_v4()));

        let (signing_input, signature) = token.rsplit_once('.').unwrap();
        let signature = Signature::from_slice(&base64url_decode(signature).unwrap()).unwrap();
        let key = VerifyingKey::from(&SigningKey::from_slice(&[0x42; 32]).unwrap());
        assert!(key.verify(signing_input.as_bytes(), &signature).is_ok());
        assert_eq!(decode(&token).0, json!({"alg": "ES256", "kid": "attacker"}));
    }

    /// RFC 7515 appendix A.1: the HS256 example, checked through `hs256`'s parts.
    #[test]
    fn an_hs256_token_carries_the_hmac_of_its_first_two_segments() {
        let token = hs256(&json!({"sub": "x"}), b"secret");

        let (signing_input, signature) = token.rsplit_once('.').unwrap();
        assert_eq!(
            base64url_decode(signature).unwrap(),
            hmac_sha256(b"secret", signing_input.as_bytes())
        );
        assert_eq!(decode(&token).0, json!({"alg": "HS256"}));
    }
}
