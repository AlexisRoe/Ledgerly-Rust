//! The teams endpoints.
//!
//! Java counterpart: `teams/internal/web/TeamsController.java`, which implements the generated `TeamsApi` and
//! `MeApi`. Each handler works in Spring's order: the path parameters, then the body (`@Valid`), then the role
//! check Java puts in `@PreAuthorize` (which runs once the arguments are resolved: not a teammate is 404, a
//! teammate with the wrong role 403), then the use case, which checks the role again under the team lock.

mod dto;
#[cfg(test)]
mod tests;

use axum::Json;
use axum::extract::{Request, State};
use axum::http::{HeaderValue, Method, StatusCode, header};
use axum::response::{IntoResponse, Response};
use ledgerly_shared::id::{InvitationId, TeamId, UserId};
use ledgerly_shared::role::Role;
use ledgerly_shared::web::{
    Body, CurrentUser, ErrorCode, PathParameters, Problem, Routes, accept, read_json, uuid_parameter,
};

use crate::application::{
    AcceptInvitation, ChangeMemberRole, CreateTeam, DeclineInvitation, GetMe, InviteMember, LeaveTeam, RemoveMember,
    RevokeInvitation, TeamAuthorization, UseCaseError,
};
use dto::{
    ChangeRoleRequest, CreateTeamRequest, InvitationResponse, InviteRequest, MeResponse, MemberResponse,
    MembershipResponse, TeamResponse,
};

/// The use cases behind the endpoints.
#[derive(Debug, Clone)]
pub(crate) struct TeamsApi {
    pub authorization: TeamAuthorization,
    pub create_team: CreateTeam,
    pub invite: InviteMember,
    pub revoke: RevokeInvitation,
    pub accept: AcceptInvitation,
    pub decline: DeclineInvitation,
    pub change_role: ChangeMemberRole,
    pub remove: RemoveMember,
    pub leave: LeaveTeam,
    pub get_me: GetMe,
}

impl TeamsApi {
    pub(crate) fn routes(self) -> Routes {
        Routes::new()
            .route(Method::GET, "/api/v1/me", Body::Ignored, me, self.clone())
            .route(Method::POST, "/api/v1/teams", Body::Json, create_team, self.clone())
            .route(
                Method::POST,
                "/api/v1/teams/{teamId}:leave",
                Body::Ignored,
                leave,
                self.clone(),
            )
            .producing(accept::PROBLEM)
            .route(
                Method::POST,
                "/api/v1/teams/{teamId}/invitations",
                Body::Json,
                invite,
                self.clone(),
            )
            .route(
                Method::DELETE,
                "/api/v1/teams/{teamId}/invitations/{invitationId}",
                Body::Ignored,
                revoke,
                self.clone(),
            )
            .producing(accept::PROBLEM)
            .route(
                Method::PATCH,
                "/api/v1/teams/{teamId}/members/{userId}",
                Body::Json,
                change_role,
                self.clone(),
            )
            .route(
                Method::DELETE,
                "/api/v1/teams/{teamId}/members/{userId}",
                Body::Ignored,
                remove,
                self.clone(),
            )
            .producing(accept::PROBLEM)
            .route(
                Method::POST,
                "/api/v1/invitations/{invitationId}:accept",
                Body::Ignored,
                accept,
                self.clone(),
            )
            .route(
                Method::POST,
                "/api/v1/invitations/{invitationId}:decline",
                Body::Ignored,
                decline,
                self,
            )
            .producing(accept::PROBLEM)
    }
}

/// `GET /me`: the caller, their role per team and their pending invitations.
async fn me(State(api): State<TeamsApi>, user: CurrentUser) -> Result<Json<MeResponse>, Problem> {
    let me = api.get_me.execute(user.id()).await.map_err(|e| failure(&e))?;
    Ok(Json(MeResponse::from(me)))
}

/// `POST /teams`: 201 with `Location`; the creator is the team's OWNER.
async fn create_team(State(api): State<TeamsApi>, user: CurrentUser, request: Request) -> Result<Response, Problem> {
    let body: CreateTeamRequest = read_json(request).await?;
    let Some(name) = body.name else {
        // Validation has required it.
        return Err(Problem::internal_error());
    };
    let team = api
        .create_team
        .execute(&name, user.id())
        .await
        .map_err(|e| failure(&e))?;
    let location = format!("/api/v1/teams/{}", team.id);
    let mut response = (StatusCode::CREATED, Json(TeamResponse::from(team))).into_response();
    if let Ok(location) = HeaderValue::from_str(&location) {
        response.headers_mut().insert(header::LOCATION, location);
    }
    Ok(response)
}

/// `POST /teams/{teamId}/invitations` (OWNER): 201, the same whether or not the address is registered.
async fn invite(
    State(api): State<TeamsApi>,
    user: CurrentUser,
    parameters: PathParameters,
    request: Request,
) -> Result<Response, Problem> {
    let team = team(&parameters)?;
    let body: InviteRequest = read_json(request).await?;
    let (Some(email), Some(role)) = (body.email, body.role) else {
        return Err(Problem::internal_error());
    };
    owner_only(&api, team, user).await?;
    let view = api
        .invite
        .execute(team, user.id(), &email, role.0)
        .await
        .map_err(|e| failure(&e))?;
    Ok((StatusCode::CREATED, Json(InvitationResponse::from(view))).into_response())
}

/// `DELETE /teams/{teamId}/invitations/{invitationId}` (OWNER): 204.
async fn revoke(
    State(api): State<TeamsApi>,
    user: CurrentUser,
    parameters: PathParameters,
) -> Result<StatusCode, Problem> {
    let team = team(&parameters)?;
    let invitation = invitation(&parameters)?;
    owner_only(&api, team, user).await?;
    api.revoke
        .execute(team, invitation, user.id())
        .await
        .map_err(|e| failure(&e))?;
    Ok(StatusCode::NO_CONTENT)
}

/// `POST /invitations/{invitationId}:accept`: 200 with the team joined and the role received.
async fn accept(
    State(api): State<TeamsApi>,
    user: CurrentUser,
    parameters: PathParameters,
) -> Result<Json<MembershipResponse>, Problem> {
    let invitation = invitation(&parameters)?;
    let joined = api
        .accept
        .execute(invitation, user.id())
        .await
        .map_err(|e| failure(&e))?;
    Ok(Json(MembershipResponse {
        team_id: joined.team,
        role: joined.role.as_str(),
    }))
}

/// `POST /invitations/{invitationId}:decline`: 204.
async fn decline(
    State(api): State<TeamsApi>,
    user: CurrentUser,
    parameters: PathParameters,
) -> Result<StatusCode, Problem> {
    let invitation = invitation(&parameters)?;
    api.decline
        .execute(invitation, user.id())
        .await
        .map_err(|e| failure(&e))?;
    Ok(StatusCode::NO_CONTENT)
}

/// `PATCH /teams/{teamId}/members/{userId}` (OWNER): 200 with the member's new role.
async fn change_role(
    State(api): State<TeamsApi>,
    user: CurrentUser,
    parameters: PathParameters,
    request: Request,
) -> Result<Json<MemberResponse>, Problem> {
    let team = team(&parameters)?;
    let member = member(&parameters)?;
    let body: ChangeRoleRequest = read_json(request).await?;
    let Some(role) = body.role else {
        return Err(Problem::internal_error());
    };
    owner_only(&api, team, user).await?;
    let role = api
        .change_role
        .execute(team, user.id(), member, role.0)
        .await
        .map_err(|e| failure(&e))?;
    Ok(Json(MemberResponse {
        user_id: member,
        role: role.as_str(),
    }))
}

/// `DELETE /teams/{teamId}/members/{userId}` (OWNER): 204.
async fn remove(
    State(api): State<TeamsApi>,
    user: CurrentUser,
    parameters: PathParameters,
) -> Result<StatusCode, Problem> {
    let team = team(&parameters)?;
    let member = member(&parameters)?;
    owner_only(&api, team, user).await?;
    api.remove
        .execute(team, user.id(), member)
        .await
        .map_err(|e| failure(&e))?;
    Ok(StatusCode::NO_CONTENT)
}

/// `POST /teams/{teamId}:leave` (any teammate): 204.
async fn leave(
    State(api): State<TeamsApi>,
    user: CurrentUser,
    parameters: PathParameters,
) -> Result<StatusCode, Problem> {
    let team = team(&parameters)?;
    api.authorization
        .at_least(team, user.id(), Role::Member)
        .await
        .map_err(|e| failure(&e))?;
    api.leave.execute(team, user.id()).await.map_err(|e| failure(&e))?;
    Ok(StatusCode::NO_CONTENT)
}

/// Java: `@PreAuthorize("@teamAuthorization.anyOf(#teamId, 'OWNER')")`.
async fn owner_only(api: &TeamsApi, team: TeamId, user: CurrentUser) -> Result<(), Problem> {
    api.authorization
        .any_of(team, user.id(), &[Role::Owner])
        .await
        .map_err(|e| failure(&e))
}

fn team(parameters: &PathParameters) -> Result<TeamId, Problem> {
    uuid_parameter(parameters, "teamId").map(TeamId::from_uuid)
}

fn invitation(parameters: &PathParameters) -> Result<InvitationId, Problem> {
    uuid_parameter(parameters, "invitationId").map(InvitationId::from_uuid)
}

fn member(parameters: &PathParameters) -> Result<UserId, Problem> {
    uuid_parameter(parameters, "userId").map(UserId::from_uuid)
}

/// The problem for a failed use case. A database failure is a bare 500, logged by kind only: a database message
/// can quote a value, such as an email.
fn failure(error: &UseCaseError) -> Problem {
    match error {
        UseCaseError::Unauthenticated => Problem::not_authenticated(),
        UseCaseError::NotFound => Problem::not_found(),
        UseCaseError::Forbidden(detail) => Problem::forbidden(detail.clone()),
        UseCaseError::EmailNotVerified => Problem::new(ErrorCode::EmailNotVerified, error.to_string()),
        UseCaseError::LastOwner(detail) => Problem::new(ErrorCode::LastOwner, *detail),
        UseCaseError::AlreadyMember => Problem::new(ErrorCode::AlreadyMember, error.to_string()),
        UseCaseError::InvitationExpired => Problem::new(ErrorCode::InvitationExpired, error.to_string()),
        UseCaseError::InvitationNotPending(detail) => Problem::new(ErrorCode::InvitationNotPending, detail.clone()),
        UseCaseError::Database(_) => {
            tracing::error!(error = "database", "Unhandled exception");
            Problem::internal_error()
        }
    }
}
