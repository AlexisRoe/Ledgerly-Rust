//! The use cases: register, verify an email, log in, refresh and log out.
//!
//! Java counterpart: `identity/internal/application/`.

mod login;
mod refresh;
mod register;
mod verify_email;

pub(crate) use login::{Login, Session};
pub(crate) use refresh::{Logout, Refresh};
pub(crate) use register::Register;
pub(crate) use verify_email::VerifyEmail;

use ledgerly_shared::text::strip;
use secrecy::SecretString;

use crate::password::{self, HashingFailed, PasswordHash};

/// Why a use case did not succeed.
#[derive(Debug, thiserror::Error)]
pub(crate) enum UseCaseError {
    /// Wrong credentials, a locked address, an unusable refresh token: Java's `Unauthenticated`. Deliberately
    /// without a reason.
    #[error("not authenticated")]
    Unauthenticated,
    /// Java's `InvalidRequest(field, message)`.
    #[error("{field}: {message}")]
    Invalid { field: &'static str, message: &'static str },
    #[error("database: {0}")]
    Database(#[from] sqlx::Error),
    /// Hashing, random numbers or signing failed. Says nothing about the inputs.
    #[error("{0}")]
    Crypto(&'static str),
}

/// Emails are compared lower-cased and trimmed, so `Ann@Example.com` and `ann@example.com` are one account.
/// Java: `User.normalise` (`strip()` and `toLowerCase(Locale.ROOT)`).
pub(crate) fn normalise_email(email: &str) -> String {
    strip(email).to_lowercase()
}

/// Hashes on Tokio's blocking pool: Argon2 takes tens of milliseconds of CPU.
async fn hash_password(password: SecretString) -> Result<PasswordHash, UseCaseError> {
    tokio::task::spawn_blocking(move || password::hash(&password))
        .await
        .map_err(|_| UseCaseError::Crypto("password hashing stopped"))?
        .map_err(|HashingFailed| UseCaseError::Crypto("password hashing failed"))
}

/// Whether `password` matches `stored`, or with `None` the time of a check that fails, on the blocking pool.
async fn password_matches(password: SecretString, stored: Option<PasswordHash>) -> bool {
    tokio::task::spawn_blocking(move || match stored {
        Some(stored) => password::matches(&password, &stored),
        None => password::match_nothing(&password),
    })
    .await
    .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emails_are_trimmed_and_lower_cased() {
        assert_eq!(normalise_email("  Ann@Example.COM\t"), "ann@example.com");
        assert_eq!(normalise_email("ÄNNE@BÜCHER.DE"), "änne@bücher.de");
    }
}
