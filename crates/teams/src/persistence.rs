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

/// A new team. Java: `TeamRepository.save(new Team(name, now))`.
pub(crate) async fn insert_team(
    connection: &mut PgConnection,
    id: TeamId,
    name: &str,
    now: DateTime<Utc>,
) -> sqlx::Result<()> {
    sqlx::query!(
        "insert into teams (id, name, created_at) values ($1, $2, $3)",
        id.as_uuid(),
        name,
        now,
    )
    .execute(connection)
    .await?;
    Ok(())
}

/// Takes the team row lock (`select … for update`), which serialises every change of who belongs to the team
/// and with which role. `false` if the team does not exist. Java: `TeamRepository.lockById`
/// (`PESSIMISTIC_WRITE`).
pub(crate) async fn lock_team(connection: &mut PgConnection, team: TeamId) -> sqlx::Result<bool> {
    let row = sqlx::query_scalar!("select id from teams where id = $1 for update", team.as_uuid())
        .fetch_optional(connection)
        .await?;
    Ok(row.is_some())
}

/// Makes `user` a member of `team`. Java: `MembershipRepository.save(new Membership(…))`.
pub(crate) async fn insert_membership(
    connection: &mut PgConnection,
    team: TeamId,
    user: UserId,
    role: Role,
    now: DateTime<Utc>,
) -> sqlx::Result<()> {
    sqlx::query!(
        "insert into memberships (team_id, user_id, role, joined_at) values ($1, $2, $3, $4)",
        team.as_uuid(),
        user.as_uuid(),
        role.as_str(),
        now,
    )
    .execute(connection)
    .await?;
    Ok(())
}

/// The user's role in the team, `None` when they are not a teammate. Java: `MembershipRepository.findById`.
pub(crate) async fn role_of(connection: &mut PgConnection, team: TeamId, user: UserId) -> sqlx::Result<Option<Role>> {
    let row = sqlx::query_scalar!(
        "select role from memberships where team_id = $1 and user_id = $2",
        team.as_uuid(),
        user.as_uuid(),
    )
    .fetch_optional(connection)
    .await?;
    row.as_deref().map(role).transpose()
}

/// How many members hold the role `OWNER`. Java: `MembershipRepository.countByIdTeamIdAndRole`.
pub(crate) async fn count_owners(connection: &mut PgConnection, team: TeamId) -> sqlx::Result<i64> {
    let count = sqlx::query_scalar!(
        r#"select count(*) as "count!" from memberships where team_id = $1 and role = 'OWNER'"#,
        team.as_uuid(),
    )
    .fetch_one(connection)
    .await?;
    Ok(count)
}

/// Sets a member's role. Java: `Membership.changeRole`, flushed at commit.
pub(crate) async fn update_role(
    connection: &mut PgConnection,
    team: TeamId,
    user: UserId,
    role: Role,
) -> sqlx::Result<()> {
    sqlx::query!(
        "update memberships set role = $3 where team_id = $1 and user_id = $2",
        team.as_uuid(),
        user.as_uuid(),
        role.as_str(),
    )
    .execute(connection)
    .await?;
    Ok(())
}

/// Ends a membership. Java: `MembershipRepository.delete`.
pub(crate) async fn delete_membership(connection: &mut PgConnection, team: TeamId, user: UserId) -> sqlx::Result<()> {
    sqlx::query!(
        "delete from memberships where team_id = $1 and user_id = $2",
        team.as_uuid(),
        user.as_uuid(),
    )
    .execute(connection)
    .await?;
    Ok(())
}

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

/// A new invitation. Java: `InvitationRepository.save(new Invitation(…))`.
pub(crate) async fn insert_invitation(
    connection: &mut PgConnection,
    invitation: &Invitation,
    invited_by: UserId,
    now: DateTime<Utc>,
) -> sqlx::Result<()> {
    sqlx::query!(
        "insert into invitations (id, team_id, email, role, status, invited_by, created_at, expires_at) \
         values ($1, $2, $3, $4, $5, $6, $7, $8)",
        invitation.id.as_uuid(),
        invitation.team.as_uuid(),
        invitation.email,
        invitation.role.as_str(),
        invitation.stored_status.as_str(),
        invited_by.as_uuid(),
        now,
        invitation.expires_at,
    )
    .execute(connection)
    .await?;
    Ok(())
}

/// Stores a new status. Java: `Invitation.accept`/`decline`/`revoke`, flushed at commit (or at once with
/// `saveAndFlush`).
pub(crate) async fn set_status(
    connection: &mut PgConnection,
    invitation: InvitationId,
    status: InvitationStatus,
) -> sqlx::Result<()> {
    sqlx::query!(
        "update invitations set status = $2 where id = $1",
        invitation.as_uuid(),
        status.as_str(),
    )
    .execute(connection)
    .await?;
    Ok(())
}

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

/// The team's invitation with this id, its row locked (`for update`); `None` also when it belongs to another
/// team. Java: `InvitationRepository.findByIdAndTeamId`, without the lock (see [`lock_invitation`]).
pub(crate) async fn lock_invitation_in_team(
    connection: &mut PgConnection,
    team: TeamId,
    id: InvitationId,
) -> sqlx::Result<Option<Invitation>> {
    sqlx::query_as!(
        InvitationRow,
        "select id, team_id, email, role, status, expires_at from invitations where id = $1 and team_id = $2 \
         for update",
        id.as_uuid(),
        team.as_uuid(),
    )
    .fetch_optional(connection)
    .await?
    .map(Invitation::try_from)
    .transpose()
}

/// The address's stored-`PENDING` invitation to the team, expired or not; there is at most one. Java:
/// `InvitationRepository.findByTeamIdAndEmailAndStatus(team, email, PENDING)`.
pub(crate) async fn find_pending_in_team(
    connection: &mut PgConnection,
    team: TeamId,
    email: &str,
) -> sqlx::Result<Option<Invitation>> {
    sqlx::query_as!(
        InvitationRow,
        "select id, team_id, email, role, status, expires_at from invitations \
         where team_id = $1 and email = $2 and status = 'PENDING'",
        team.as_uuid(),
        email,
    )
    .fetch_optional(connection)
    .await?
    .map(Invitation::try_from)
    .transpose()
}

/// The invitation with this id, of any team: only for its addressee, who must be checked against
/// [`Invitation::email`] before anything is revealed. Java: `InvitationRepository.findById` in
/// `InvitationResponses.load`.
pub(crate) async fn find_invitation_for_addressee(
    connection: &mut PgConnection,
    id: InvitationId,
) -> sqlx::Result<Option<Invitation>> {
    sqlx::query_as!(
        InvitationRow,
        "select id, team_id, email, role, status, expires_at from invitations where id = $1",
        id.as_uuid(),
    )
    .fetch_optional(connection)
    .await?
    .map(Invitation::try_from)
    .transpose()
}

/// [`find_invitation_for_addressee`] with the row locked (`for update`), so that accepting, declining and revoking
/// the same invitation run one after the other and only the first succeeds. Java reads without a lock, so two
/// of them can both succeed (decided after M6: lock). Every use case that takes the team lock takes it first.
pub(crate) async fn lock_invitation(
    connection: &mut PgConnection,
    id: InvitationId,
) -> sqlx::Result<Option<Invitation>> {
    sqlx::query_as!(
        InvitationRow,
        "select id, team_id, email, role, status, expires_at from invitations where id = $1 for update",
        id.as_uuid(),
    )
    .fetch_optional(connection)
    .await?
    .map(Invitation::try_from)
    .transpose()
}

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
