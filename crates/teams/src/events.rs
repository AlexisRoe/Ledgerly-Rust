//! The teams module's events. Ids and role names only, never an email or a team name (Java's ADR 0001).
//!
//! Java counterpart: `teams/events/TeamEvent.java` (the sealed `TeamEvent` and its eight records).

use chrono::{DateTime, Utc};
use ledgerly_shared::events::Event;
use ledgerly_shared::id::{InvitationId, TeamId, UserId};
use ledgerly_shared::role::Role;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Everything that changes a team. Java: the sealed `TeamEvent`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TeamEvent {
    pub event_id: Uuid,
    /// The team the event happened in.
    pub team_id: TeamId,
    /// The user who did it.
    pub actor_id: UserId,
    /// When it happened, by the application clock.
    pub occurred_at: DateTime<Utc>,
    pub kind: TeamEventKind,
}

/// What happened. Java: one record per variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TeamEventKind {
    /// A team was created; its creator became OWNER. Java: `TeamCreated`.
    TeamCreated,
    /// An OWNER invited an email address with the offered role. Java: `MemberInvited`.
    MemberInvited { invitation_id: InvitationId, role: Role },
    /// An OWNER withdrew a pending invitation. Java: `InvitationRevoked`.
    InvitationRevoked { invitation_id: InvitationId },
    /// The invited person accepted and became a member. Java: `InvitationAccepted`.
    InvitationAccepted { invitation_id: InvitationId },
    /// The invited person declined. Java: `InvitationDeclined`.
    InvitationDeclined { invitation_id: InvitationId },
    /// An OWNER changed a member's role. Java: `MemberRoleChanged`.
    MemberRoleChanged {
        member_id: UserId,
        old_role: Role,
        new_role: Role,
    },
    /// An OWNER removed a member. Java: `MemberRemoved`.
    MemberRemoved { member_id: UserId },
    /// A member left on their own. Java: `MemberLeft`.
    MemberLeft,
}

impl TeamEvent {
    pub(crate) fn new(team_id: TeamId, actor_id: UserId, occurred_at: DateTime<Utc>, kind: TeamEventKind) -> Self {
        Self {
            event_id: Uuid::now_v7(),
            team_id,
            actor_id,
            occurred_at,
            kind,
        }
    }
}

impl Event for TeamEvent {
    fn event_id(&self) -> Uuid {
        self.event_id
    }

    fn occurred_at(&self) -> DateTime<Utc> {
        self.occurred_at
    }

    fn event_type(&self) -> &'static str {
        match self.kind {
            TeamEventKind::TeamCreated => "ledgerly-rust:teams.TeamCreated",
            TeamEventKind::MemberInvited { .. } => "ledgerly-rust:teams.MemberInvited",
            TeamEventKind::InvitationRevoked { .. } => "ledgerly-rust:teams.InvitationRevoked",
            TeamEventKind::InvitationAccepted { .. } => "ledgerly-rust:teams.InvitationAccepted",
            TeamEventKind::InvitationDeclined { .. } => "ledgerly-rust:teams.InvitationDeclined",
            TeamEventKind::MemberRoleChanged { .. } => "ledgerly-rust:teams.MemberRoleChanged",
            TeamEventKind::MemberRemoved { .. } => "ledgerly-rust:teams.MemberRemoved",
            TeamEventKind::MemberLeft => "ledgerly-rust:teams.MemberLeft",
        }
    }

    fn payload(&self) -> serde_json::Result<String> {
        serde_json::to_string(self)
    }
}
