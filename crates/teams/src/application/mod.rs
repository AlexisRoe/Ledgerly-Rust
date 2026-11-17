//! The use cases.
//!
//! Java counterpart: `teams/internal/application/`.
//!
//! Every use case that changes who belongs to a team, or with which role, takes the team row lock first
//! ([`guard`]) and checks the caller's role after it, inside its transaction. The web layer has checked the
//! role already ([`TeamAuthorization`]), but a concurrent request may have changed it since.

mod authorization;
#[cfg(test)]
mod concurrency_tests;
mod create_team;
mod get_me;
mod guard;
mod invitations;
mod members;

use std::sync::Arc;

use ledgerly_shared::clock::Clock;
use ledgerly_shared::events::Publisher;
use sqlx::PgPool;

pub(crate) use authorization::TeamAuthorization;
pub(crate) use create_team::{CreateTeam, CreatedTeam};
pub(crate) use get_me::{GetMe, Me};
pub(crate) use invitations::{AcceptInvitation, DeclineInvitation, InvitationView, InviteMember, RevokeInvitation};
pub(crate) use members::{ChangeMemberRole, LeaveTeam, RemoveMember};

/// What every use case that writes needs.
#[derive(Debug, Clone)]
pub(crate) struct Context {
    pub pool: PgPool,
    pub clock: Arc<dyn Clock>,
    pub events: Arc<dyn Publisher>,
}

/// Why a use case did not succeed. The details are Java's.
#[derive(Debug, thiserror::Error)]
pub(crate) enum UseCaseError {
    /// The caller's account no longer exists. Java: `Unauthenticated`.
    #[error("not authenticated")]
    Unauthenticated,
    /// Not a teammate, or not addressed to the caller: the same 404 as "does not exist". Java: `NotFound`.
    #[error("not found")]
    NotFound,
    /// A teammate whose role may not do this. Java: `Forbidden(detail)`.
    #[error("{0}")]
    Forbidden(String),
    /// Java: `Forbidden(EMAIL_NOT_VERIFIED, …)`.
    #[error("Verify your email address before accepting")]
    EmailNotVerified,
    /// Java: `Conflict(LAST_OWNER, detail)`.
    #[error("{0}")]
    LastOwner(&'static str),
    /// Java: `Conflict(ALREADY_MEMBER, …)`.
    #[error("You are already a teammate")]
    AlreadyMember,
    /// Java: `Conflict(INVITATION_EXPIRED, …)`.
    #[error("This invitation has expired")]
    InvitationExpired,
    /// Java: `Conflict(INVITATION_NOT_PENDING, detail)`.
    #[error("{0}")]
    InvitationNotPending(String),
    #[error("database: {0}")]
    Database(#[from] sqlx::Error),
}
