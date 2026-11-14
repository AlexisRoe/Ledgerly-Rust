//! The tables of Ledgerly-Java's V4 migration: `teams`, `memberships`, `invitations`.
//!
//! Java counterpart: `teams/internal/infrastructure/{TeamRepository,MembershipRepository,InvitationRepository}`
//! and the JPA mappings in `internal/domain/`.
//!
//! Every lookup of team data takes the team id (OWASP API1). The two exceptions are lookups for the person
//! an invitation is addressed to, who is not yet a teammate: by invitation id (and then checked against their
//! email) and by email (for `/me`, only once the email is verified).
//!
//! The functions take a `&mut PgConnection`, which a pool connection and an open transaction both deref to, so
//! the caller decides the transaction boundary.

use chrono::{DateTime, Utc};
use ledgerly_shared::id::{InvitationId, TeamId, UserId};
use ledgerly_shared::role::Role;
use sqlx::PgConnection;

use crate::domain::{Invitation, InvitationStatus};

/// A role column that holds something other than a role name: a broken row, reported as a decode error.
fn role(text: &str) -> sqlx::Result<Role> {
    text.parse().map_err(|error| sqlx::Error::Decode(Box::new(error)))
}

fn status(text: &str) -> sqlx::Result<InvitationStatus> {
    text.parse().map_err(|error| sqlx::Error::Decode(Box::new(error)))
}

/// Takes the team row lock (`select … for update`), which serialises every change of who belongs to the team
/// and with which role. `false` if the team does not exist. Java: `TeamRepository.lockById`
/// A team the user belongs to, with its name and their role.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TeamRole {
    pub team: TeamId,
    pub name: String,
    pub role: Role,
}

/// Every team the user belongs to, in the order they joined. Java: `MembershipRepository.findByIdUserId` and
/// `TeamRepository.findAllById`, which name no order; this one names the order Postgres usually returns.
pub(crate) async fn teams_of(connection: &mut PgConnection, user: UserId) -> sqlx::Result<Vec<TeamRole>> {
    let rows = sqlx::query!(
        "select t.id, t.name, m.role from memberships m join teams t on t.id = m.team_id \
         where m.user_id = $1 order by m.joined_at, m.team_id",
        user.as_uuid(),
    )
    .fetch_all(connection)
    .await?;
    rows.into_iter()
        .map(|row| {
            Ok(TeamRole {
                team: TeamId::from_uuid(row.id),
                name: row.name,
                role: role(&row.role)?,
            })
        })
        .collect()
}

/// Stores a new status. Java: `Invitation.accept`/`decline`/`revoke`, flushed at commit (or at once with
struct InvitationRow {
    id: uuid::Uuid,
    team_id: uuid::Uuid,
    email: String,
    role: String,
    status: String,
    expires_at: DateTime<Utc>,
}

impl TryFrom<InvitationRow> for Invitation {
    type Error = sqlx::Error;

    fn try_from(row: InvitationRow) -> sqlx::Result<Self> {
        Ok(Self {
            id: InvitationId::from_uuid(row.id),
            team: TeamId::from_uuid(row.team_id),
            email: row.email,
            role: role(&row.role)?,
            stored_status: status(&row.status)?,
            expires_at: row.expires_at,
        })
    }
}

/// The team's invitation with this id; `None` also when it belongs to another team. Java:
/// The address's stored-`PENDING` invitation to the team, expired or not; there is at most one. Java:
/// The invitation with this id, of any team: only for its addressee, who must be checked against
/// [`Invitation::email`] before anything is revealed. Java: `InvitationRepository.findById` in
/// A stored-`PENDING` invitation to an address, with the inviting team's name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OpenInvitation {
    pub invitation: Invitation,
    pub team_name: String,
}

/// Every stored-`PENDING` invitation sent to the (normalised) address, expired or not, oldest first. Java:
/// `InvitationRepository.findByEmailAndStatus(email, PENDING)` and `TeamRepository.findAllById`, which name no
/// order.
pub(crate) async fn pending_for_email(connection: &mut PgConnection, email: &str) -> sqlx::Result<Vec<OpenInvitation>> {
    let rows = sqlx::query!(
        "select i.id, i.team_id, i.email, i.role, i.status, i.expires_at, t.name as team_name \
         from invitations i join teams t on t.id = i.team_id \
         where i.email = $1 and i.status = 'PENDING' order by i.created_at, i.id",
        email,
    )
    .fetch_all(connection)
    .await?;
    rows.into_iter()
        .map(|row| {
            let invitation = Invitation::try_from(InvitationRow {
                id: row.id,
                team_id: row.team_id,
                email: row.email,
                role: row.role,
                status: row.status,
                expires_at: row.expires_at,
            })?;
            Ok(OpenInvitation {
                invitation,
                team_name: row.team_name,
            })
        })
        .collect()
}
