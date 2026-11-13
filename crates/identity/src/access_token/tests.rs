use std::sync::Arc;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chrono::TimeZone;
use ledgerly_shared::clock::FixedClock;
use serde_json::json;

use super::*;

/// A P-256 pair made with `openssl ecparam -name prime256v1 -genkey | openssl pkcs8 -topk8 -nocrypt` for these
/// tests only. It signs nothing anywhere else.
const PRIVATE_PEM: &str = "-----BEGIN PRIVATE KEY-----
MIGHAgEAMBMGByqGSM49AgEGCCqGSM49AwEHBG0wawIBAQQghfNuvg8nrWXfE7hv
mI/Noikbvn3xA38pMmwyehHxeOGhRANCAARBwG65F2infxmSX0aKuTqf6Vblahf7
Yu2qqHGwR8kh7A9Juw8JdkWTbGIoeUoX8oyPMs/RhmF90ZBG/YZMQ7EU
-----END PRIVATE KEY-----
";
const PUBLIC_PEM: &str = "-----BEGIN PUBLIC KEY-----
MFkwEwYHKoZIzj0CAQYIKoZIzj0DAQcDQgAEQcBuuRdop38Zkl9Girk6n+lW5WoX
+2LtqqhxsEfJIewPSbsPCXZFk2xiKHlKF/KMjzLP0YZhfdGQRv2GTEOxFA==
-----END PUBLIC KEY-----
";
/// The pair's coordinates and RFC 7638 thumbprint, computed outside Rust: the last 64 bytes of the DER public
/// key, Base64url, and SHA-256 over `{"crv":"P-256","kty":"EC","x":…,"y":…}` (Python's `hashlib`).
const X: &str = "QcBuuRdop38Zkl9Girk6n-lW5WoX-2LtqqhxsEfJIew";
const Y: &str = "D0m7Dwl2RZNsYih5ShfyjI8yz9GGYX3RkEb9hkxDsRQ";
const THUMBPRINT: &str = "28wJw-TUrqRb_2oVFZP5-pfyFr1uoxy4-7lmB68ZLT0";

fn fixed_keys() -> SigningKeys {
    SigningKeys::from_config(Some(&SecretString::from(PRIVATE_PEM)), Some(PUBLIC_PEM), false).unwrap()
}

fn noon() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 11, 13, 12, 0, 0).unwrap()
}

struct Fixture {
    clock: Arc<FixedClock>,
    tokens: AccessTokens,
}

fn fixture() -> Fixture {
    let clock = Arc::new(FixedClock::at(noon()));
    let tokens = AccessTokens::new(Arc::new(fixed_keys()), Duration::from_secs(600), clock.clone());
    Fixture { clock, tokens }
}

fn issue(tokens: &AccessTokens, user: UserId) -> String {
    tokens.issue(user).unwrap().token.expose_secret().to_owned()
}

fn part(token: &str, index: usize) -> Value {
    let segment = token.split('.').nth(index).unwrap();
    serde_json::from_slice(&URL_SAFE_NO_PAD.decode(segment).unwrap()).unwrap()
}

/// `header.claims.signature`, signed with the fixed key: for tokens that are well signed but wrong otherwise.
fn signed(header: &Value, claims: &Value) -> String {
    signed_text(&header.to_string(), &claims.to_string())
}

fn signed_text(header: &str, claims: &str) -> String {
    let input = format!("{}.{}", URL_SAFE_NO_PAD.encode(header), URL_SAFE_NO_PAD.encode(claims));
    let signature: Signature = fixed_keys().signing.sign(input.as_bytes());
    format!("{input}.{}", URL_SAFE_NO_PAD.encode(signature.to_bytes()))
}

fn header() -> Value {
    json!({"kid": THUMBPRINT, "alg": "ES256"})
}

fn claims(user: UserId) -> Value {
    let now = noon().timestamp();
    json!({"sub": user.to_string(), "exp": now + 600, "iat": now, "jti": Uuid::new_v4()})
}

// --- keys ---

#[test]
fn the_kid_of_a_configured_key_is_its_rfc_7638_thumbprint() {
    let keys = fixed_keys();

    assert_eq!(keys.kid(), THUMBPRINT);
    assert_eq!(
        serde_json::to_value(keys.public_jwk()).unwrap(),
        json!({"kty": "EC", "crv": "P-256", "kid": THUMBPRINT, "x": X, "y": Y})
    );
}

#[test]
fn the_jwk_has_javas_member_order_and_no_private_part() {
    let text = serde_json::to_string(&fixed_keys().public_jwk()).unwrap();

    assert_eq!(
        text,
        format!(r#"{{"kty":"EC","crv":"P-256","kid":"{THUMBPRINT}","x":"{X}","y":"{Y}"}}"#)
    );
    assert!(!text.contains("\"d\""));
}

#[test]
fn keys_may_be_bare_base64_without_the_pem_armour() {
    let bare = |pem: &str| {
        pem.lines()
            .filter(|line| !line.starts_with("-----"))
            .collect::<String>()
    };

    let keys = SigningKeys::from_config(
        Some(&SecretString::from(bare(PRIVATE_PEM))),
        Some(&bare(PUBLIC_PEM)),
        false,
    )
    .unwrap();

    assert_eq!(keys.kid(), THUMBPRINT);
}

#[test]
fn whitespace_anywhere_in_a_key_is_ignored_as_in_java() {
    let spaced = PUBLIC_PEM.replace('\n', " \r\n\t");

    let keys = SigningKeys::from_config(Some(&SecretString::from(PRIVATE_PEM)), Some(&spaced), false).unwrap();

    assert_eq!(keys.kid(), THUMBPRINT);
}

#[test]
fn without_keys_local_and_test_generate_one_with_a_random_v4_kid() {
    let blank = SecretString::from(" ");
    for (private_key, public_key) in [
        (None, None),
        (Some(&blank), Some(PUBLIC_PEM)),
        (Some(&blank), Some(" ")),
    ] {
        let first = SigningKeys::from_config(private_key, public_key, true).unwrap();
        let second = SigningKeys::from_config(private_key, public_key, true).unwrap();

        assert_eq!(first.kid().parse::<Uuid>().unwrap().get_version_num(), 4);
        assert_ne!(first.kid(), second.kid());
        assert_ne!(first.public_jwk(), second.public_jwk());
    }
}

#[test]
fn without_keys_elsewhere_the_app_does_not_start() {
    let error = SigningKeys::from_config(None, Some(PUBLIC_PEM), false).unwrap_err();

    assert!(matches!(error, KeyError::Missing));
    assert_eq!(
        error.to_string(),
        "ledgerly.auth.jwt-private-key and jwt-public-key must be set outside the local and test profiles"
    );
}

#[test]
fn an_unreadable_key_is_refused_without_quoting_it() {
    let private_key = SecretString::from(PRIVATE_PEM);
    for (private_key, public_key) in [
        (SecretString::from("not base64 at all!"), PUBLIC_PEM),
        // Each key in the other's place.
        (SecretString::from(PUBLIC_PEM), PRIVATE_PEM),
        (private_key.clone(), "AAAA"),
    ] {
        let error = SigningKeys::from_config(Some(&private_key), Some(public_key), true).unwrap_err();

        assert!(matches!(error, KeyError::Invalid));
        assert_eq!(error.to_string(), "Invalid JWT key configuration");
    }
}

#[test]
fn debug_shows_the_kid_and_nothing_of_the_key() {
    let printed = format!("{:?}", fixed_keys());

    assert_eq!(printed, format!("SigningKeys {{ kid: {THUMBPRINT:?}, .. }}"));
}

// --- issuing ---

#[test]
fn a_token_has_javas_header_and_only_sub_exp_iat_and_jti() {
    let Fixture { tokens, .. } = fixture();
    let user = UserId::new_v7();

    let issued = tokens.issue(user).unwrap();
    let token = issued.token.expose_secret();

    let header_text = String::from_utf8(URL_SAFE_NO_PAD.decode(token.split('.').next().unwrap()).unwrap()).unwrap();
    assert_eq!(header_text, format!(r#"{{"kid":"{THUMBPRINT}","alg":"ES256"}}"#));
    let claims = part(token, 1);
    let names: Vec<&str> = claims.as_object().unwrap().keys().map(String::as_str).collect();
    assert_eq!(names, ["exp", "iat", "jti", "sub"]);
    assert_eq!(claims["sub"], user.to_string());
    assert_eq!(claims["iat"], noon().timestamp());
    assert_eq!(claims["exp"], noon().timestamp() + 600);
    let jti: Uuid = claims["jti"].as_str().unwrap().parse().unwrap();
    assert_eq!(jti.get_version_num(), 4);
    assert_eq!(issued.expires_in_seconds, 600);
}

#[test]
fn the_claims_are_written_in_nimbus_order() {
    let Fixture { tokens, .. } = fixture();

    let token = issue(&tokens, UserId::new_v7());

    let text = String::from_utf8(URL_SAFE_NO_PAD.decode(token.split('.').nth(1).unwrap()).unwrap()).unwrap();
    let positions: Vec<usize> = ["\"sub\"", "\"exp\"", "\"iat\"", "\"jti\""]
        .iter()
        .map(|name| text.find(name).unwrap())
        .collect();
    assert!(positions.is_sorted(), "{text}");
}

#[test]
fn timestamps_are_whole_seconds_rounded_down_and_ten_minutes_apart() {
    let Fixture { clock, tokens } = fixture();
    clock.set(noon() + TimeDelta::milliseconds(999));

    let claims = part(&issue(&tokens, UserId::new_v7()), 1);

    assert_eq!(claims["iat"], noon().timestamp());
    assert_eq!(claims["exp"], noon().timestamp() + 600);
}

#[test]
fn the_signature_is_es256_over_header_and_claims_with_the_public_key() {
    let Fixture { tokens, .. } = fixture();
    let token = issue(&tokens, UserId::new_v7());

    let (input, signature) = token.rsplit_once('.').unwrap();
    let signature = Signature::from_slice(&URL_SAFE_NO_PAD.decode(signature).unwrap()).unwrap();
    let public = VerifyingKey::from_public_key_der(&pem_body(PUBLIC_PEM).unwrap()).unwrap();

    assert!(public.verify(input.as_bytes(), &signature).is_ok());
}

// --- verifying ---

#[test]
fn our_own_token_names_its_user() {
    let Fixture { tokens, .. } = fixture();
    let user = UserId::new_v7();

    assert_eq!(tokens.verify(&issue(&tokens, user)), Ok(user));
}

#[test]
fn a_token_lives_ten_minutes_plus_thirty_seconds_of_skew() {
    let Fixture { clock, tokens } = fixture();
    let user = UserId::new_v7();
    let token = issue(&tokens, user);

    clock.set(noon() + TimeDelta::seconds(630));
    assert_eq!(tokens.verify(&token), Ok(user));

    clock.set(noon() + TimeDelta::seconds(630) + TimeDelta::milliseconds(1));
    assert_eq!(tokens.verify(&token), Err(InvalidToken));

    // Java: `AuthApiTest.expiredTokenGets401`.
    clock.set(noon() + TimeDelta::minutes(11));
    assert_eq!(tokens.verify(&token), Err(InvalidToken));
}

#[test]
fn not_before_is_honoured_with_the_same_skew() {
    let Fixture { tokens, .. } = fixture();
    let user = UserId::new_v7();
    let mut early = claims(user);

    early["nbf"] = json!(noon().timestamp() + 30);
    assert_eq!(tokens.verify(&signed(&header(), &early)), Ok(user));

    early["nbf"] = json!(noon().timestamp() + 31);
    assert_eq!(tokens.verify(&signed(&header(), &early)), Err(InvalidToken));
}

#[test]
fn exp_iat_and_jti_are_optional_as_in_spring() {
    let Fixture { tokens, .. } = fixture();
    let user = UserId::new_v7();

    let token = signed(&header(), &json!({"sub": user.to_string()}));

    assert_eq!(tokens.verify(&token), Ok(user));
}

#[test]
fn a_token_without_kid_is_checked_against_our_key() {
    let Fixture { tokens, .. } = fixture();
    let user = UserId::new_v7();

    assert_eq!(
        tokens.verify(&signed(&json!({"alg": "ES256"}), &claims(user))),
        Ok(user)
    );
    assert_eq!(
        tokens.verify(&signed(&json!({"alg": "ES256", "typ": "JWT"}), &claims(user))),
        Ok(user)
    );
}

#[test]
fn a_tampered_token_is_refused() {
    let Fixture { tokens, .. } = fixture();
    let token = issue(&tokens, UserId::new_v7());
    let segments: Vec<&str> = token.split('.').collect();
    let mut other = part(&token, 1);
    other["sub"] = json!(UserId::new_v7().to_string());

    let tampered = format!(
        "{}.{}.{}",
        segments[0],
        URL_SAFE_NO_PAD.encode(other.to_string()),
        segments[2]
    );

    assert_eq!(tokens.verify(&tampered), Err(InvalidToken));
}

#[test]
fn alg_none_hs256_and_other_algorithms_are_refused() {
    let Fixture { tokens, .. } = fixture();
    let claims = claims(UserId::new_v7());
    let unsigned = format!(
        "{}.{}.",
        URL_SAFE_NO_PAD.encode(r#"{"alg":"none"}"#),
        URL_SAFE_NO_PAD.encode(claims.to_string())
    );

    assert_eq!(tokens.verify(&unsigned), Err(InvalidToken));
    for alg in ["none", "HS256", "ES384", "RS256", "es256", "ES256 "] {
        // Correctly signed with our key, but declaring another algorithm.
        let token = signed(&json!({"alg": alg}), &claims);
        assert_eq!(tokens.verify(&token), Err(InvalidToken), "{alg}");
    }
}

#[test]
fn a_token_signed_with_another_key_is_refused() {
    let Fixture { tokens, .. } = fixture();
    let attacker = SigningKey::from_slice(&[0x42; 32]).unwrap();
    let input = format!(
        "{}.{}",
        URL_SAFE_NO_PAD.encode(r#"{"alg":"ES256"}"#),
        URL_SAFE_NO_PAD.encode(claims(UserId::new_v7()).to_string())
    );
    let signature: Signature = attacker.sign(input.as_bytes());

    let token = format!("{input}.{}", URL_SAFE_NO_PAD.encode(signature.to_bytes()));

    assert_eq!(tokens.verify(&token), Err(InvalidToken));
}

#[test]
fn headers_that_nimbus_finds_no_key_for_are_refused() {
    let Fixture { tokens, .. } = fixture();
    let claims = claims(UserId::new_v7());

    for header in [
        json!({"alg": "ES256", "kid": "another"}),
        json!({"alg": "ES256", "kid": 1}),
        json!({"alg": "ES256", "crit": ["exp"]}),
        json!({"alg": "ES256", "x5t#S256": "abc"}),
        json!({"alg": "ES256", "typ": "at+jwt"}),
        json!({"kid": THUMBPRINT}),
    ] {
        assert_eq!(tokens.verify(&signed(&header, &claims)), Err(InvalidToken), "{header}");
    }
}

#[test]
fn claims_of_the_wrong_type_or_without_a_user_id_are_refused() {
    let Fixture { tokens, .. } = fixture();
    let user = UserId::new_v7().to_string();
    let now = noon().timestamp();

    for claims in [
        json!({"exp": now + 600}),
        json!({"sub": 42}),
        json!({"sub": "alice"}),
        json!({"sub": "1-1-1-1-1"}),
        json!({"sub": user, "exp": (now + 600).to_string()}),
        json!({"sub": user, "iat": "now"}),
        json!({"sub": user, "jti": 7}),
        json!({"sub": user, "aud": [1]}),
        json!({"sub": user, "iss": false}),
        json!(["not", "an", "object"]),
    ] {
        assert_eq!(
            tokens.verify(&signed(&header(), &claims)),
            Err(InvalidToken),
            "{claims}"
        );
    }
}

#[test]
fn a_repeated_claim_is_refused_like_gson_refuses_it() {
    let Fixture { tokens, .. } = fixture();
    let user = UserId::new_v7();
    let claims = format!(r#"{{"sub":"{user}","sub":"{}"}}"#, UserId::new_v7());

    assert_eq!(
        tokens.verify(&signed_text(&header().to_string(), &claims)),
        Err(InvalidToken)
    );
}

#[test]
fn garbage_is_refused() {
    let Fixture { tokens, .. } = fixture();
    let token = issue(&tokens, UserId::new_v7());

    for garbage in [
        "",
        "abc",
        "a.b.c",
        "....",
        "Bearer",
        &format!("{token}.extra"),
        &token[..token.len() - 4],
        &format!("{}.", token.rsplit_once('.').unwrap().0),
    ] {
        assert_eq!(tokens.verify(garbage), Err(InvalidToken), "{garbage}");
    }
}

#[test]
fn a_token_of_a_generated_key_does_not_verify_with_another_generated_key() {
    let clock: Arc<dyn Clock> = Arc::new(FixedClock::at(noon()));
    let one = AccessTokens::new(
        Arc::new(SigningKeys::from_config(None, None, true).unwrap()),
        Duration::from_secs(600),
        clock.clone(),
    );
    let other = AccessTokens::new(
        Arc::new(SigningKeys::from_config(None, None, true).unwrap()),
        Duration::from_secs(600),
        clock,
    );

    assert_eq!(other.verify(&issue(&one, UserId::new_v7())), Err(InvalidToken));
}
