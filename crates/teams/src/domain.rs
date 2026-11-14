//! Invitations and the rules that need no database: pure, no I/O.
//!
//! Java counterpart: `teams/internal/domain/` (`Invitation`, `InvitationStatus`, `Team`, `Membership`).

use std::fmt;
use std::str::FromStr;

use chrono::{DateTime, Utc};
use ledgerly_shared::id::{InvitationId, TeamId};
use ledgerly_shared::role::Role;
use ledgerly_shared::text::strip;

/// Where an invitation is in its life; only [`InvitationStatus::Pending`] can still be answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InvitationStatus {
    Pending,
    Accepted,
    Declined,
    Revoked,
    /// Never stored: computed from the clock when an invitation is read.
    Expired,
}

/// The text is not the name of an [`InvitationStatus`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("Unknown invitation status: {0}")]
pub(crate) struct UnknownStatus(String);

impl InvitationStatus {
    const ALL: [Self; 5] = [
        Self::Pending,
        Self::Accepted,
        Self::Declined,
        Self::Revoked,
        Self::Expired,
    ];

    /// The name as Java, the database and the API write it.
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "PENDING",
            Self::Accepted => "ACCEPTED",
            Self::Declined => "DECLINED",
            Self::Revoked => "REVOKED",
            Self::Expired => "EXPIRED",
        }
    }
}

impl fmt::Display for InvitationStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for InvitationStatus {
    type Err = UnknownStatus;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|status| status.as_str() == s)
            .ok_or_else(|| UnknownStatus(s.to_owned()))
    }
}

/// An owner's offer to an email address to join a team with a role. Java: `Invitation`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Invitation {
    pub id: InvitationId,
    pub team: TeamId,
    /// Normalised, see [`normalise_email`].
    pub email: String,
    pub role: Role,
    /// As stored: never [`InvitationStatus::Expired`].
    pub stored_status: InvitationStatus,
    pub expires_at: DateTime<Utc>,
}

impl Invitation {
    /// The status at `now`. Expiry is computed from the clock, so no scheduler has to flip a flag: a pending
    /// invitation whose `expires_at` is not after `now` is expired. Java: `Invitation.status(now)`.
    pub(crate) fn status(&self, now: DateTime<Utc>) -> InvitationStatus {
        if self.stored_status == InvitationStatus::Pending && self.expires_at <= now {
            InvitationStatus::Expired
        } else {
            self.stored_status
        }
    }
}

/// Trims and lower-cases an address so it can be compared with an account's email. Java: `Invitation.normalise`
/// (`strip()` and `toLowerCase(Locale.ROOT)`), the same rule as identity's.
pub(crate) fn normalise_email(email: &str) -> String {
    strip(email).to_lowercase()
}

/// The team's name as stored: stripped. Java: `new Team(name, now)`.
pub(crate) fn team_name(name: &str) -> String {
    strip(name).to_owned()
}

#[cfg(test)]
mod tests {
    use chrono::TimeDelta;

    use super::*;

    fn invitation(stored_status: InvitationStatus, expires_at: DateTime<Utc>) -> Invitation {
        Invitation {
            id: InvitationId::new_v7(),
            team: TeamId::new_v7(),
            email: "ann@example.com".to_owned(),
            role: Role::Member,
            stored_status,
            expires_at,
        }
    }

    #[test]
    fn a_pending_invitation_expires_at_its_expiry_instant() {
        let expires_at = DateTime::parse_from_rfc3339("2026-11-21T10:00:00Z").unwrap().to_utc();
        let pending = invitation(InvitationStatus::Pending, expires_at);

        assert_eq!(
            pending.status(expires_at - TimeDelta::microseconds(1)),
            InvitationStatus::Pending
        );
        assert_eq!(pending.status(expires_at), InvitationStatus::Expired);
        assert_eq!(
            pending.status(expires_at + TimeDelta::days(1)),
            InvitationStatus::Expired
        );
    }

    #[test]
    fn an_answered_invitation_keeps_its_status_after_expiry() {
        let expires_at = DateTime::parse_from_rfc3339("2026-11-21T10:00:00Z").unwrap().to_utc();
        for status in [
            InvitationStatus::Accepted,
            InvitationStatus::Declined,
            InvitationStatus::Revoked,
        ] {
            assert_eq!(
                invitation(status, expires_at).status(expires_at + TimeDelta::days(1)),
                status
            );
        }
    }

    #[test]
    fn statuses_are_javas_enum_constants() {
        for status in InvitationStatus::ALL {
            assert_eq!(status.as_str().parse::<InvitationStatus>(), Ok(status));
        }
        assert!("pending".parse::<InvitationStatus>().is_err());
    }

    #[test]
    fn emails_are_trimmed_and_lower_cased() {
        assert_eq!(normalise_email("  Ann@Example.COM\t"), "ann@example.com");
    }

    #[test]
    fn team_names_are_stripped() {
        assert_eq!(team_name("  Acme \n"), "Acme");
    }
}
