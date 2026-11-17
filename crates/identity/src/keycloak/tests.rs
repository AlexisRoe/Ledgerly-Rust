//! The provider's token check without a provider: tokens signed here with a P-256 key, checked against a key
//! set built from it. Java's counterparts are the validators of `KeycloakResourceServerConfig`; signatures and
//! keys from a real Keycloak (RS256) are checked by the parity suite (`keycloak.rs`).

use std::sync::Arc;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chrono::{TimeZone, Utc};
use ledgerly_shared::clock::{Clock, FixedClock};
use ring::rand::SystemRandom;
use ring::signature::{ECDSA_P256_SHA256_FIXED_SIGNING, EcdsaKeyPair, KeyPair};
use serde_json::{Value, json};

use super::*;

const ISSUER: &str = "http://keycloak.test/realms/ledgerly";
const NOW: i64 = 1_795_424_400; // 2026-11-17T09:00:00Z

struct Signer {
    pair: EcdsaKeyPair,
    rng: SystemRandom,
}

impl Signer {
    fn new() -> Self {
        let rng = SystemRandom::new();
        let pkcs8 = EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, &rng).unwrap();
        let pair = EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, pkcs8.as_ref(), &rng).unwrap();
        Self { pair, rng }
    }

    fn jwk(&self, kid: &str) -> Value {
        let public = self.pair.public_key().as_ref();
        json!({"kid": kid, "kty": "EC", "crv": "P-256", "alg": "ES256", "use": "sig",
               "x": URL_SAFE_NO_PAD.encode(&public[1..33]), "y": URL_SAFE_NO_PAD.encode(&public[33..])})
    }

    fn sign(&self, header: &Value, claims: &Value) -> String {
        let input = format!(
            "{}.{}",
            URL_SAFE_NO_PAD.encode(header.to_string()),
            URL_SAFE_NO_PAD.encode(claims.to_string())
        );
        let signature = self.pair.sign(&self.rng, input.as_bytes()).unwrap();
        format!("{input}.{}", URL_SAFE_NO_PAD.encode(signature.as_ref()))
    }
}

fn keys(set: &Value) -> Keys {
    let keys = jwk::parse_set(set).unwrap();
    Keys {
        algorithms: jwk::algorithms(&keys).into(),
        keys: keys.into(),
    }
}

/// A verifier whose provider and database are never asked: only [`ExternalTokens::decode`] is called. The
/// lazy pool needs a Tokio runtime to be made, hence `#[tokio::test]`.
fn tokens() -> ExternalTokens {
    let clock: Arc<dyn Clock> = Arc::new(FixedClock::at(Utc.timestamp_opt(NOW, 0).unwrap()));
    let pool = sqlx::PgPool::connect_lazy("postgres://unused@127.0.0.1:1/unused").unwrap();
    let events: Arc<dyn ledgerly_shared::events::Publisher> = Arc::new(NoEvents);
    let provision = Provision::new(pool, clock.clone(), events);
    ExternalTokens::new(ISSUER, "ledgerly-api", clock, provision).unwrap()
}

#[derive(Debug)]
struct NoEvents;

impl ledgerly_shared::events::Publisher for NoEvents {
    fn publish<'a>(
        &'a self,
        _connection: &'a mut sqlx::PgConnection,
        _event: &'a dyn ledgerly_shared::events::Event,
    ) -> std::pin::Pin<Box<dyn Future<Output = sqlx::Result<()>> + Send + 'a>> {
        Box::pin(std::future::ready(Ok(())))
    }
}

fn header() -> Value {
    json!({"alg": "ES256", "typ": "JWT", "kid": "k1"})
}

/// Claims as Keycloak writes them for the `ledgerly-web` client with the audience mapper.
fn claims() -> Value {
    json!({
        "exp": NOW + 300, "iat": NOW, "jti": "f0e1", "iss": ISSUER, "aud": ["ledgerly-api", "account"],
        "sub": "8d3c-kc-subject", "typ": "Bearer", "azp": "ledgerly-web",
        "email_verified": true, "email": "alice@example.com"
    })
}

fn with(mut claims: Value, name: &str, value: Value) -> Value {
    claims[name] = value;
    claims
}

fn without(mut claims: Value, name: &str) -> Value {
    claims.as_object_mut().unwrap().remove(name);
    claims
}

#[tokio::test]
async fn a_token_of_the_provider_for_this_api_names_its_person() {
    let signer = Signer::new();
    let keys = keys(&json!({"keys": [signer.jwk("k1")]}));

    let person = tokens().decode(&signer.sign(&header(), &claims()), &keys);

    assert_eq!(
        person,
        Ok(Person {
            subject: "8d3c-kc-subject".to_owned(),
            email: Some("alice@example.com".to_owned()),
            email_verified: true,
        })
    );
}

#[tokio::test]
async fn a_single_audience_string_is_an_audience_too() {
    let signer = Signer::new();
    let keys = keys(&json!({"keys": [signer.jwk("k1")]}));

    let token = signer.sign(&header(), &with(claims(), "aud", json!("ledgerly-api")));

    assert!(tokens().decode(&token, &keys).is_ok());
}

/// `KeycloakEndToEndTest.aTokenForAnotherApplicationOfTheSameRealmIsRefused`.
#[tokio::test]
async fn a_token_for_another_audience_or_from_another_issuer_is_refused() {
    let signer = Signer::new();
    let keys = keys(&json!({"keys": [signer.jwk("k1")]}));
    let refused = |claims: Value| tokens().decode(&signer.sign(&header(), &claims), &keys);

    assert_eq!(
        refused(with(claims(), "aud", json!(["account"]))),
        Err(Refusal::Invalid)
    );
    assert_eq!(
        refused(with(claims(), "aud", json!("other-app"))),
        Err(Refusal::Invalid)
    );
    assert_eq!(refused(without(claims(), "aud")), Err(Refusal::Invalid));
    assert_eq!(
        refused(with(claims(), "iss", json!("http://keycloak.test/realms/other"))),
        Err(Refusal::Invalid)
    );
    assert_eq!(
        refused(with(claims(), "iss", json!(format!("{ISSUER}/")))),
        Err(Refusal::Invalid)
    );
    assert_eq!(refused(without(claims(), "iss")), Err(Refusal::Invalid));
}

/// `JwtTimestampValidator(Duration.ofSeconds(30))` on the application clock.
#[tokio::test]
async fn expiry_and_not_before_have_thirty_seconds_of_skew() {
    let signer = Signer::new();
    let keys = keys(&json!({"keys": [signer.jwk("k1")]}));
    let decode = |claims: Value| tokens().decode(&signer.sign(&header(), &claims), &keys);

    assert!(decode(with(claims(), "exp", json!(NOW - 30))).is_ok());
    assert_eq!(decode(with(claims(), "exp", json!(NOW - 31))), Err(Refusal::Invalid));
    assert!(decode(with(claims(), "nbf", json!(NOW + 30))).is_ok());
    assert_eq!(decode(with(claims(), "nbf", json!(NOW + 31))), Err(Refusal::Invalid));
}

/// `KeycloakEndToEndTest.tamperedAndGarbageTokensAreRefused`.
#[tokio::test]
async fn tampered_and_garbage_tokens_are_refused() {
    let signer = Signer::new();
    let keys = keys(&json!({"keys": [signer.jwk("k1")]}));
    let token = signer.sign(&header(), &claims());
    let parts: Vec<&str> = token.split('.').collect();
    let tampered = format!(
        "{}.{}.{}",
        parts[0],
        URL_SAFE_NO_PAD.encode(with(claims(), "sub", json!("someone-else")).to_string()),
        parts[2]
    );
    let foreign = Signer::new().sign(&header(), &claims());

    for token in [tampered.as_str(), foreign.as_str(), "not.a.jwt", "", "a.b", "a.b.c.d"] {
        assert_eq!(tokens().decode(token, &keys), Err(Refusal::Invalid), "{token}");
    }
}

#[tokio::test]
async fn only_the_algorithms_of_the_key_set_are_accepted() {
    let signer = Signer::new();
    let keys = keys(&json!({"keys": [signer.jwk("k1")]}));
    let decode = |header: Value| tokens().decode(&signer.sign(&header, &claims()), &keys);

    assert_eq!(decode(json!({"alg": "RS256", "kid": "k1"})), Err(Refusal::Invalid));
    assert_eq!(decode(json!({"alg": "none", "kid": "k1"})), Err(Refusal::Invalid));
    assert_eq!(decode(json!({"alg": "HS256", "kid": "k1"})), Err(Refusal::Invalid));
    assert_eq!(decode(json!({"kid": "k1"})), Err(Refusal::Invalid));
}

#[tokio::test]
async fn the_header_is_read_as_nimbus_reads_it() {
    let signer = Signer::new();
    let keys = keys(&json!({"keys": [signer.jwk("k1")]}));
    let decode = |header: Value| tokens().decode(&signer.sign(&header, &claims()), &keys);

    assert!(decode(json!({"alg": "ES256"})).is_ok(), "no kid: any key of the type");
    assert!(decode(json!({"alg": "ES256", "kid": "k1", "typ": "jose"})).is_ok());
    assert_eq!(decode(json!({"alg": "ES256", "typ": "at+jwt"})), Err(Refusal::Invalid));
    assert_eq!(decode(json!({"alg": "ES256", "crit": ["exp"]})), Err(Refusal::Invalid));
    assert_eq!(decode(json!({"alg": "ES256", "kid": 1})), Err(Refusal::Invalid));
}

/// A key that is not in the set yet: the set may have been rotated since it was fetched.
#[tokio::test]
async fn a_token_naming_an_unknown_key_asks_for_the_keys_again() {
    let (old, new) = (Signer::new(), Signer::new());
    let keys = keys(&json!({"keys": [old.jwk("k1")]}));

    let token = new.sign(&json!({"alg": "ES256", "kid": "k2"}), &claims());

    assert_eq!(tokens().decode(&token, &keys), Err(Refusal::NoKey));
}

#[tokio::test]
async fn the_person_is_read_as_the_subject_converter_reads_it() {
    let signer = Signer::new();
    let keys = keys(&json!({"keys": [signer.jwk("k1")]}));
    let decode = |claims: Value| tokens().decode(&signer.sign(&header(), &claims), &keys);

    let unverified = decode(without(without(claims(), "email"), "email_verified")).unwrap();
    assert_eq!((unverified.email, unverified.email_verified), (None, false));
    assert!(
        decode(with(claims(), "email_verified", json!("TRUE")))
            .unwrap()
            .email_verified
    );
    assert!(
        !decode(with(claims(), "email_verified", json!("yes")))
            .unwrap()
            .email_verified
    );
    assert_eq!(decode(without(claims(), "sub")), Err(Refusal::Invalid));
    assert_eq!(decode(with(claims(), "sub", json!(42))), Err(Refusal::Invalid));
}

/// `ExternalIdentityTest.aMissingSubjectIsRefused`: refused before the database is asked.
#[tokio::test]
async fn a_blank_subject_is_refused() {
    let signer = Signer::new();
    let keys = keys(&json!({"keys": [signer.jwk("k1")]}));

    for subject in ["", " ", "\t"] {
        let token = signer.sign(&header(), &with(claims(), "sub", json!(subject)));
        assert_eq!(tokens().decode(&token, &keys), Err(Refusal::Invalid), "{subject:?}");
    }
}
