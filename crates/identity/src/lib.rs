//! The identity module: users, passwords, email verification, refresh tokens, JWTs and the JWKS endpoint.
//!
//! Java counterpart: `src/main/java/com/ledgerly/identity/` in Ledgerly-Java at the commit in
//! `vendor/ledgerly-java/SOURCE`.

#![forbid(unsafe_code)]

mod access_token;
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "register, login and refresh use it from M5.3")
)]
mod opaque_token;
#[cfg_attr(not(test), expect(dead_code, reason = "register and login use it from M5.3"))]
mod password;
mod strict_json;
mod web;

use std::sync::Arc;

use ledgerly_shared::clock::Clock;
use ledgerly_shared::config::{AuthConfig, Profiles};
use ledgerly_shared::web::Routes;

pub use crate::access_token::{AccessTokens, InvalidToken, KeyError, SigningKeys};

/// The identity module as the app wires it.
#[derive(Debug, Clone)]
pub struct Identity {
    /// `None` with `keycloak`: the identity provider issues the tokens, and Ledgerly signs none (Java's
    /// `@OwnTokenIssuing` beans are not created).
    tokens: Option<AccessTokens>,
}

impl Identity {
    /// Reads or generates the signing key. Java generates one only in the `local` and `test` profiles.
    ///
    /// # Errors
    ///
    /// The [`KeyError`] that stops the application from starting.
    pub fn new(auth: &AuthConfig, profiles: Profiles, clock: Arc<dyn Clock>) -> Result<Self, KeyError> {
        if profiles.keycloak {
            return Ok(Self { tokens: None });
        }
        let keys = SigningKeys::from_config(
            auth.jwt_private_key.as_ref(),
            auth.jwt_public_key.as_deref(),
            profiles.local || profiles.test,
        )?;
        Ok(Self {
            tokens: Some(AccessTokens::new(Arc::new(keys), auth.access_token_ttl, clock)),
        })
    }

    /// The access tokens Ledgerly signs, if it signs any.
    #[must_use]
    pub const fn access_tokens(&self) -> Option<&AccessTokens> {
        self.tokens.as_ref()
    }

    /// The identity endpoints: none with `keycloak`.
    #[must_use]
    pub fn routes(&self) -> Routes {
        self.tokens.as_ref().map_or_else(Routes::new, web::routes)
    }
}
