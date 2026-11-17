//! The JSON bodies of the teams endpoints, as `api/openapi.yaml` describes them.
//!
//! Java counterpart: the models generated into `com.ledgerly.shared.api.model` (`MeResponse`, `MeTeam`,
//! `MeInvitation`, …) and the mapping in `teams/internal/web/TeamsController`. Properties are declared in the
//! spec's order, which is the order Jackson writes them in.

use ledgerly_shared::id::{InvitationId, TeamId, UserId};
use ledgerly_shared::iso::format_instant;
use ledgerly_shared::role::Role;
use ledgerly_shared::web::{Validate, Violations};
use serde::{Deserialize, Deserializer, Serialize};

use crate::application::{CreatedTeam, InvitationView, Me};

/// A role name. Anything else is a body Jackson could not read (the generated enum's `fromValue` throws): 400
/// `MALFORMED_REQUEST`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RoleBody(pub Role);

impl<'de> Deserialize<'de> for RoleBody {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let name = String::deserialize(deserializer)?;
        name.parse().map(Self).map_err(serde::de::Error::custom)
    }
}

/// `CreateTeamRequest`: a name of 1 to 100 characters.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CreateTeamRequest {
    pub name: Option<String>,
}

impl Validate for CreateTeamRequest {
    fn validate(&self, violations: &mut Violations) {
        violations.require("name", self.name.as_ref());
        violations.size("name", self.name.as_deref(), 1, 100);
    }
}

/// `InviteRequest`: email `format: email`, at most 254; a role.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct InviteRequest {
    pub email: Option<String>,
    pub role: Option<RoleBody>,
}

impl Validate for InviteRequest {
    fn validate(&self, violations: &mut Violations) {
        violations.require("email", self.email.as_ref());
        violations.size("email", self.email.as_deref(), 0, 254);
        violations.email("email", self.email.as_deref());
        violations.require("role", self.role.as_ref());
    }
}

/// `ChangeRoleRequest`: a role.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ChangeRoleRequest {
    pub role: Option<RoleBody>,
}

impl Validate for ChangeRoleRequest {
    fn validate(&self, violations: &mut Violations) {
        violations.require("role", self.role.as_ref());
    }
}

/// `TeamResponse`.
#[derive(Debug, Serialize)]
pub(crate) struct TeamResponse {
    id: TeamId,
    name: String,
}

impl From<CreatedTeam> for TeamResponse {
    fn from(team: CreatedTeam) -> Self {
        Self {
            id: team.id,
            name: team.name,
        }
    }
}

/// `InvitationResponse`.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct InvitationResponse {
    id: InvitationId,
    role: &'static str,
    status: &'static str,
    expires_at: String,
}

impl From<InvitationView> for InvitationResponse {
    fn from(view: InvitationView) -> Self {
        Self {
            id: view.id,
            role: view.role.as_str(),
            status: view.status.as_str(),
            expires_at: format_instant(view.expires_at),
        }
    }
}

/// `MemberResponse`.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MemberResponse {
    pub user_id: UserId,
    pub role: &'static str,
}

/// `MembershipResponse`.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MembershipResponse {
    pub team_id: TeamId,
    pub role: &'static str,
}

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
