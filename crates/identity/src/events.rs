//! The identity module's events. None carries a team, an email or a token.
//!
//! Java counterpart: `identity/events/` (`IdentityEvent` and its three records). Java's module-internal
//! `VerificationLinkIssued` is not an event here: the `local` log line it fed is written by `Register` itself.

use chrono::{DateTime, Utc};
use ledgerly_shared::events::Event;
use ledgerly_shared::id::UserId;
use uuid::Uuid;

/// Everything identity publishes. Java: the sealed `IdentityEvent`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdentityEvent {
    /// An account was created. Java: `UserRegistered`.
    UserRegistered {
        event_id: Uuid,
        actor_id: UserId,
        occurred_at: DateTime<Utc>,
    },
    /// An account's email was confirmed. Java: `UserVerified`.
    UserVerified {
        event_id: Uuid,
        actor_id: UserId,
        occurred_at: DateTime<Utc>,
    },
    /// A refresh token was presented again after it had been rotated; its family is revoked. Java:
    /// `RefreshTokenReuseDetected`.
    RefreshTokenReuseDetected {
        event_id: Uuid,
        actor_id: UserId,
        family_id: Uuid,
        occurred_at: DateTime<Utc>,
    },
}

impl IdentityEvent {
    pub(crate) fn user_registered(actor_id: UserId, occurred_at: DateTime<Utc>) -> Self {
        Self::UserRegistered {
            event_id: Uuid::now_v7(),
            actor_id,
            occurred_at,
        }
    }

    pub(crate) fn user_verified(actor_id: UserId, occurred_at: DateTime<Utc>) -> Self {
        Self::UserVerified {
            event_id: Uuid::now_v7(),
            actor_id,
            occurred_at,
        }
    }

    pub(crate) fn refresh_token_reuse_detected(actor_id: UserId, family_id: Uuid, occurred_at: DateTime<Utc>) -> Self {
        Self::RefreshTokenReuseDetected {
            event_id: Uuid::now_v7(),
            actor_id,
            family_id,
            occurred_at,
        }
    }

    /// The user who acted. Java: `IdentityEvent.actorId()`.
    #[must_use]
    pub const fn actor_id(&self) -> UserId {
        match self {
            Self::UserRegistered { actor_id, .. }
            | Self::UserVerified { actor_id, .. }
            | Self::RefreshTokenReuseDetected { actor_id, .. } => *actor_id,
        }
    }
}

impl Event for IdentityEvent {
    fn event_id(&self) -> Uuid {
        match self {
            Self::UserRegistered { event_id, .. }
            | Self::UserVerified { event_id, .. }
            | Self::RefreshTokenReuseDetected { event_id, .. } => *event_id,
        }
    }

    fn occurred_at(&self) -> DateTime<Utc> {
        match self {
            Self::UserRegistered { occurred_at, .. }
            | Self::UserVerified { occurred_at, .. }
            | Self::RefreshTokenReuseDetected { occurred_at, .. } => *occurred_at,
        }
    }

    fn event_type(&self) -> &'static str {
        match self {
            Self::UserRegistered { .. } => "ledgerly-rust:identity.UserRegistered",
            Self::UserVerified { .. } => "ledgerly-rust:identity.UserVerified",
            Self::RefreshTokenReuseDetected { .. } => "ledgerly-rust:identity.RefreshTokenReuseDetected",
        }
    }
}
