//! The request and response bodies of `/auth/*`, with the constraints of Java's generated models.
//!
//! Passwords and tokens are `SecretString`s from the moment they are parsed: they cannot be printed or logged
//! by accident and are wiped when dropped.

use ledgerly_shared::web::{Validate, Violations};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};

/// `RegisterRequest`: email `format: email`, at most 254; password 12 to 128.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RegisterRequest {
    pub email: Option<String>,
    pub password: Option<SecretString>,
}

impl Validate for RegisterRequest {
    fn validate(&self, violations: &mut Violations) {
        email(violations, self.email.as_deref());
        password(violations, self.password.as_ref(), 12);
    }
}

/// `LoginRequest`: as `RegisterRequest`, but any password of 1 to 128 characters. Checked before anything is
/// hashed, so an over-long password costs no Argon2 run.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct LoginRequest {
    pub email: Option<String>,
    pub password: Option<SecretString>,
}

impl Validate for LoginRequest {
    fn validate(&self, violations: &mut Violations) {
        email(violations, self.email.as_deref());
        password(violations, self.password.as_ref(), 1);
    }
}

/// `VerifyRequest`: a token of 20 to 200 characters.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct VerifyRequest {
    pub token: Option<SecretString>,
}

impl Validate for VerifyRequest {
    fn validate(&self, violations: &mut Violations) {
        violations.require("token", self.token.as_ref());
        violations.size("token", self.token.as_ref().map(ExposeSecret::expose_secret), 20, 200);
    }
}

fn email(violations: &mut Violations, email: Option<&str>) {
    violations.require("email", email);
    violations.size("email", email, 0, 254);
    violations.email("email", email);
}

fn password(violations: &mut Violations, password: Option<&SecretString>, min: usize) {
    violations.require("password", password);
    violations.size("password", password.map(ExposeSecret::expose_secret), min, 128);
}

/// `Accepted`: the one answer to every registration.
#[derive(Debug, Serialize)]
pub(crate) struct Accepted {
    pub message: &'static str,
}

/// `TokenResponse`.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TokenResponse<'a> {
    pub access_token: &'a str,
    pub token_type: &'static str,
    pub expires_in: i64,
}
