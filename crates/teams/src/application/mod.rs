//! The use cases.
//!
//! Java counterpart: `teams/internal/application/`.

mod get_me;

pub(crate) use get_me::{GetMe, Me};

/// Why a use case did not succeed.
#[derive(Debug, thiserror::Error)]
pub(crate) enum UseCaseError {
    /// The caller's account no longer exists. Java: `Unauthenticated`.
    #[error("not authenticated")]
    Unauthenticated,
    #[error("database: {0}")]
    Database(#[from] sqlx::Error),
}
