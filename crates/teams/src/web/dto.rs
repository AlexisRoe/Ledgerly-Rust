//! The JSON bodies of the teams endpoints, as `api/openapi.yaml` describes them.
//!
//! Java counterpart: the models generated into `com.ledgerly.shared.api.model` (`MeResponse`, `MeTeam`,
//! `MeInvitation`, …) and the mapping in `teams/internal/web/TeamsController`. Properties are declared in the
//! spec's order, which is the order Jackson writes them in.

use ledgerly_shared::id::{InvitationId, TeamId, UserId};
use ledgerly_shared::iso::format_instant;
use serde::Serialize;

use crate::application::Me;

/// `MeResponse`.
#[derive(Debug, Serialize)]
pub(crate) struct MeResponse {
    id: UserId,
    email: String,
    verified: bool,
    teams: Vec<MeTeam>,
    invitations: Vec<MeInvitation>,
}

/// `MeTeam`.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct MeTeam {
    team_id: TeamId,
    name: String,
    role: &'static str,
}

/// `MeInvitation`.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct MeInvitation {
    id: InvitationId,
    team_id: TeamId,
    team_name: String,
    role: &'static str,
    expires_at: String,
}

impl From<Me> for MeResponse {
    fn from(me: Me) -> Self {
        Self {
            id: me.id,
            email: me.email,
            verified: me.verified,
            teams: me
                .teams
                .into_iter()
                .map(|team| MeTeam {
                    team_id: team.team,
                    name: team.name,
                    role: team.role.as_str(),
                })
                .collect(),
            invitations: me
                .invitations
                .into_iter()
                .map(|invitation| MeInvitation {
                    id: invitation.id,
                    team_id: invitation.team,
                    team_name: invitation.team_name,
                    role: invitation.role.as_str(),
                    expires_at: format_instant(invitation.expires_at),
                })
                .collect(),
        }
    }
}
