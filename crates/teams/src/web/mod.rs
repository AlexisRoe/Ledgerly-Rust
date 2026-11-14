//! The teams endpoints.
//!
//! Java counterpart: `teams/internal/web/TeamsController.java`, which implements the generated `TeamsApi` and
//! `MeApi`.

mod dto;
#[cfg(test)]
mod tests;

use axum::Json;
use axum::extract::State;
use axum::http::Method;
use ledgerly_shared::web::{Body, CurrentUser, Problem, Routes};

use crate::application::{GetMe, UseCaseError};
use dto::MeResponse;

/// The use cases behind the endpoints.
#[derive(Debug, Clone)]
pub(crate) struct TeamsApi {
    pub get_me: GetMe,
}

impl TeamsApi {
    pub(crate) fn routes(self) -> Routes {
        Routes::new().route(Method::GET, "/api/v1/me", Body::Ignored, me, self)
    }
}

/// `GET /me`: the caller, their role per team and their pending invitations.
async fn me(State(api): State<TeamsApi>, user: CurrentUser) -> Result<Json<MeResponse>, Problem> {
    let me = api.get_me.execute(user.id()).await.map_err(|e| failure(&e))?;
    Ok(Json(MeResponse::from(me)))
}

/// The problem for a failed use case. A database failure is a bare 500, logged by kind only: a database message
/// can quote a value, such as an email.
fn failure(error: &UseCaseError) -> Problem {
    match error {
        UseCaseError::Unauthenticated => Problem::not_authenticated(),
        UseCaseError::Database(_) => {
            tracing::error!(error = "database", "Unhandled exception");
            Problem::internal_error()
        }
    }
}
