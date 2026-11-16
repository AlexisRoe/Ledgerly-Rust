//! `GET /api/v1/teams/{teamId}/audit-events`. Java counterpart: `audit/internal/web/AuditController.java` and
//! `audit/internal/application/GetAuditEvents.java`.
//!
//! In Spring's order:
//! 1. the arguments are converted: a `teamId` that is not a UUID or a `limit` that is not an integer is 400;
//! 2. `@PreAuthorize("@teamAuthorization.anyOf(#teamId, 'OWNER')")`: not a teammate is 404, another role 403;
//! 3. the spec's constraints (`limit` 1 to 100, `after` at most 200 characters), checked by the method
//!    validation that the generated interface's `@Validated` adds behind the security advice: 400
//!    `VALIDATION_FAILED`;
//! 4. `GetAuditEvents`: the configured page size, then the cursor (400 for one that does not decode).

use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use axum::http::Method;
use ledgerly_shared::id::TeamId;
use ledgerly_shared::iso::format_instant;
use ledgerly_shared::paging::Cursor;
use ledgerly_shared::role::{Role, TeamRoles};
use ledgerly_shared::text::is_blank;
use ledgerly_shared::web::{
    Body, CurrentUser, PathParameters, Problem, QueryParameters, Routes, Violations, uuid_parameter,
};
use serde::Serialize;
use sqlx::PgPool;
use uuid::Uuid;

use crate::details::Details;
use crate::trail::{self, Row};

/// What the endpoint needs.
#[derive(Debug, Clone)]
pub(crate) struct AuditApi {
    pub pool: PgPool,
    pub roles: Arc<dyn TeamRoles>,
    pub max_page_size: u32,
}

impl AuditApi {
    pub(crate) fn routes(self) -> Routes {
        Routes::new().route(
            Method::GET,
            "/api/v1/teams/{teamId}/audit-events",
            Body::Ignored,
            list,
            self,
        )
    }
}

/// `AuditEvent`, in the spec's order. The actor and the subject are left out when the row has none, as the
/// spec describes them ("absent when the system did"); Java writes `null` (the reviewer's decision after M8
/// for batch results, applied here too).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AuditEventResponse {
    id: Uuid,
    event_type: String,
    occurred_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    actor_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    subject_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    subject_id: Option<Uuid>,
    details: Details,
}

impl From<Row> for AuditEventResponse {
    fn from(row: Row) -> Self {
        Self {
            id: row.id,
            event_type: row.event_type,
            occurred_at: format_instant(row.occurred_at),
            actor_id: row.actor_id,
            subject_type: row.subject_type,
            subject_id: row.subject_id,
            details: row.details.0,
        }
    }
}

/// `AuditEventPage`. `nextCursor` is absent on the last page.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AuditEventPage {
    items: Vec<AuditEventResponse>,
    #[serde(skip_serializing_if = "Option::is_none")]
    next_cursor: Option<String>,
}

async fn list(
    State(api): State<AuditApi>,
    user: CurrentUser,
    parameters: PathParameters,
    query: QueryParameters,
) -> Result<Json<AuditEventPage>, Problem> {
    let team = uuid_parameter(&parameters, "teamId").map(TeamId::from_uuid)?;
    let limit = query.integer_or("limit", 50)?;
    let after = query.text("after");

    let mut connection = api.pool.acquire().await.map_err(database)?;
    match api
        .roles
        .role_of(&mut connection, team, user.id())
        .await
        .map_err(database)?
    {
        None => return Err(Problem::not_found()),
        Some(Role::Owner) => {}
        Some(other) => return Err(Problem::forbidden(format!("Role {other} may not do this"))),
    }

    let mut violations = Violations::default();
    if limit < 1 {
        violations.add("limit", "must be greater than or equal to 1");
    }
    if limit > 100 {
        violations.add("limit", "must be less than or equal to 100");
    }
    violations.size("after", after.as_deref(), 0, 200);
    violations.into_result()?;

    // Java: `GetAuditEvents.execute`.
    let maximum = api.max_page_size;
    let limit = u32::try_from(limit)
        .ok()
        .filter(|limit| (1..=maximum).contains(limit))
        .ok_or_else(|| Problem::invalid_request("limit", format!("limit must be between 1 and {maximum}")))?;
    let cursor = match after.as_deref() {
        Some(token) if !is_blank(token) => Some(Cursor::decode(token)?),
        _ => None,
    };
    let page = trail::page(&mut connection, team, cursor, limit)
        .await
        .map_err(database)?;
    Ok(Json(AuditEventPage {
        items: page.items.into_iter().map(AuditEventResponse::from).collect(),
        next_cursor: page.next.map(|cursor| cursor.encode()),
    }))
}

/// A bare 500, logged by kind only.
fn database(_error: sqlx::Error) -> Problem {
    tracing::error!(error = "database", "Unhandled exception");
    Problem::internal_error()
}
