//! The `keycloak` profile: Ledgerly as a resource server of an identity provider, which issues the tokens.
//!
//! Java counterpart: `identity/internal/infrastructure/KeycloakResourceServerConfig.java` (the decoder and its
//! validators) and `identity/internal/application/ExternalSubjectJwtConverter.java` (the seam that replaces
//! the provider's `sub` with Ledgerly's user id, so that nothing outside identity knows about Keycloak).
//!
//! A token is accepted when, in this order:
//! 1. the provider can be asked (discovery and keys, [`issuer`]); if not, the token is refused (401) with one
//!    warning in the log: "Cannot verify a token: the identity provider is not reachable (…)";
//! 2. it is a JWS that one of the provider's keys signed, with an accepted algorithm ([`jwk`]);
//! 3. its claims pass Spring's validators: timestamps with 30 s of skew on the application clock, `iss`
//!    equal to the configured issuer, and `aud` containing the configured audience (`ledgerly-api`);
//! 4. it has a subject, and provisioning finds or creates its user ([`crate::application::Provision`]).

mod issuer;
mod jwk;

use std::fmt;
use std::sync::Arc;

use ledgerly_shared::clock::Clock;
use ledgerly_shared::web::{Checking, TokenCheck, TokenVerifier};
use serde_json::{Map, Value};

pub(crate) use self::issuer::TlsSetupFailed;
use self::issuer::{Issuer, Keys};
use self::jwk::{Algorithm, Wanted};
use crate::access_token::{self, InvalidToken, LENIENT_BASE64URL};
use crate::application::{Provision, ProvisioningError};

use base64::Engine;

/// Checks the identity provider's tokens and maps them to Ledgerly users.
pub(crate) struct ExternalTokens {
    issuer: Issuer,
    audience: String,
    clock: Arc<dyn Clock>,
    provision: Provision,
}

impl fmt::Debug for ExternalTokens {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ExternalTokens")
            .field("issuer", &self.issuer.issuer())
            .field("audience", &self.audience)
            .finish_non_exhaustive()
    }
}

/// A verified token's person, as the provider names them.
#[derive(Debug, PartialEq, Eq)]
struct Person {
    subject: String,
    email: Option<String>,
    email_verified: bool,
}

/// Why a token was not verified.
#[derive(Debug, PartialEq, Eq)]
enum Refusal {
    Invalid,
    /// No key of the set fits the header: the set may have changed since it was fetched.
    NoKey,
}

impl From<InvalidToken> for Refusal {
    fn from(InvalidToken: InvalidToken) -> Self {
        Self::Invalid
    }
}

impl ExternalTokens {
    pub(crate) fn new(
        issuer: &str,
        audience: &str,
        clock: Arc<dyn Clock>,
        provision: Provision,
    ) -> Result<Self, TlsSetupFailed> {
        Ok(Self {
            issuer: Issuer::new(issuer)?,
            audience: audience.to_owned(),
            clock,
            provision,
        })
    }

    async fn check(&self, token: &str) -> TokenCheck {
        let Ok(person) = self.person(token).await else {
            return TokenCheck::Invalid;
        };
        match self
            .provision
            .resolve(&person.subject, person.email.as_deref(), person.email_verified)
            .await
        {
            Ok(user) => TokenCheck::Valid(user),
            Err(ProvisioningError::Refused) => TokenCheck::Invalid,
            Err(error) => {
                tracing::error!("Cannot provision the user of a token: {error}");
                TokenCheck::Failed
            }
        }
    }

    /// Steps 1 to 3: the token's person, if the provider signed it for this API and it is current.
    async fn person(&self, token: &str) -> Result<Person, Refusal> {
        let keys = self.issuer.keys().await.map_err(|reason| {
            tracing::warn!("Cannot verify a token: the identity provider is not reachable ({reason})");
            Refusal::Invalid
        })?;
        match self.decode(token, &keys) {
            Err(Refusal::NoKey) => match self.issuer.refetch().await {
                Some(keys) => self.decode(token, &keys),
                None => Err(Refusal::Invalid),
            },
            other => other,
        }
    }

    /// Nimbus' `DefaultJWTProcessor` with Spring's validators, against the keys at hand.
    fn decode(&self, token: &str, keys: &Keys) -> Result<Person, Refusal> {
        let mut segments = token.split('.');
        let (Some(header), Some(payload), Some(signature), None) =
            (segments.next(), segments.next(), segments.next(), segments.next())
        else {
            return Err(Refusal::Invalid);
        };
        let header_json = access_token::object(header)?;
        let claims = access_token::object(payload)?;
        let signature = LENIENT_BASE64URL.decode(signature).map_err(|_| Refusal::Invalid)?;
        let wanted = wanted(&header_json, keys)?;
        let signing_input = token.get(..header.len() + 1 + payload.len()).ok_or(Refusal::Invalid)?;
        let candidates: Vec<_> = keys.keys.iter().filter(|key| key.matches(wanted)).collect();
        if candidates.is_empty() {
            return Err(Refusal::NoKey);
        }
        if !candidates
            .iter()
            .any(|key| key.verifies(wanted.algorithm, signing_input.as_bytes(), &signature))
        {
            return Err(Refusal::Invalid);
        }
        access_token::check_claim_types(&claims)?;
        access_token::check_time(self.clock.now(), &claims)?;
        self.check_issuer_and_audience(&claims)?;
        person(&claims)
    }

    /// `JwtIssuerValidator` and `KeycloakResourceServerConfig.AudienceValidator`.
    fn check_issuer_and_audience(&self, claims: &Map<String, Value>) -> Result<(), Refusal> {
        if claims.get("iss").and_then(Value::as_str) != Some(self.issuer.issuer()) {
            return Err(Refusal::Invalid);
        }
        let audience = self.audience.as_str();
        let meant_for_us = match claims.get("aud") {
            Some(Value::String(one)) => one == audience,
            Some(Value::Array(all)) => all.iter().any(|one| one.as_str() == Some(audience)),
            _ => false,
        };
        if meant_for_us { Ok(()) } else { Err(Refusal::Invalid) }
    }
}

/// The header's algorithm, which the key set must allow, and the key it names. Nimbus refuses `crit` and
/// takes `typ` `JWT` or `JOSE` or none, as for Ledgerly's own tokens.
fn wanted<'a>(header: &'a Map<String, Value>, keys: &Keys) -> Result<Wanted<'a>, Refusal> {
    let algorithm = header
        .get("alg")
        .and_then(Value::as_str)
        .and_then(Algorithm::named)
        .filter(|algorithm| keys.algorithms.contains(algorithm))
        .ok_or(Refusal::Invalid)?;
    match header.get("typ") {
        None => {}
        Some(Value::String(typ)) if typ.eq_ignore_ascii_case("JWT") || typ.eq_ignore_ascii_case("JOSE") => {}
        Some(_) => return Err(Refusal::Invalid),
    }
    if header.contains_key("crit") {
        return Err(Refusal::Invalid);
    }
    let text = |name: &str| match header.get(name) {
        None => Ok(None),
        Some(Value::String(value)) => Ok(Some(value.as_str())),
        Some(_) => Err(Refusal::Invalid),
    };
    Ok(Wanted {
        algorithm,
        kid: text("kid")?,
        thumbprint: text("x5t#S256")?,
    })
}

/// `ExternalSubjectJwtConverter`: `sub`, `email` (Spring's `getClaimAsString`) and `email_verified`
/// (`Boolean.TRUE.equals(getClaimAsBoolean(…))`).
fn person(claims: &Map<String, Value>) -> Result<Person, Refusal> {
    // A blank subject is refused before anything is looked up.
    let subject = match claims.get("sub") {
        Some(Value::String(subject)) if !ledgerly_shared::text::is_blank(subject) => subject.clone(),
        _ => return Err(Refusal::Invalid),
    };
    let email = match claims.get("email") {
        None | Some(Value::Null) => None,
        Some(Value::String(email)) => Some(email.clone()),
        Some(other) => Some(other.to_string()),
    };
    let email_verified = match claims.get("email_verified") {
        Some(Value::Bool(verified)) => *verified,
        Some(Value::String(verified)) => verified.eq_ignore_ascii_case("true"),
        _ => false,
    };
    Ok(Person {
        subject,
        email,
        email_verified,
    })
}

impl TokenVerifier for ExternalTokens {
    fn verify<'a>(&'a self, token: &'a str) -> Checking<'a> {
        Box::pin(self.check(token))
    }
}

#[cfg(test)]
mod tests;
