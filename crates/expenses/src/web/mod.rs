//! The expense endpoints: routes, request and response types, and the problems they answer with (ADR 0014).
//!
//! Java counterpart: `expenses/internal/web/ExpensesController.java`. Each handler reads its arguments in the
//! order Java's generated `ExpensesApi` declares them, because Spring stops at the first one it cannot resolve:
//! the caller, then headers and path parameters, then the body, then the constraints on headers (checked by
//! Spring's method validation once all arguments are there), then what the controller itself parses.

mod dto;
mod problem;
#[cfg(test)]
mod tests;

use std::sync::Arc;

use axum::extract::{Request, State};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, header};
use axum::response::{IntoResponse, Response};
use ledgerly_shared::clock::Clock;
use ledgerly_shared::id::{ExpenseId, TeamId};
use ledgerly_shared::text::strip;
use ledgerly_shared::version::{Version, Versioned};
use ledgerly_shared::web::{
    Body, CurrentUser, PathParameters, Problem, Routes, Violations, header_text, read_json, uuid_parameter,
};
use sqlx::PgPool;

use self::dto::{CreateExpenseRequest, ExpenseResponse, RejectRequest, UpdateExpenseRequest};
use crate::application::{
    CreateExpense, CreateExpenseCommand, DeleteExpense, GetExpense, MembershipLookup, TransitionExpense, UpdateExpense,
    UpdateExpenseCommand,
};
use crate::domain::{Action, Expense, NewExpense};
use crate::receipt_checks::ReceiptChecks;

const EXPENSES: &str = "/api/v1/teams/{teamId}/expenses";
const EXPENSE: &str = "/api/v1/teams/{teamId}/expenses/{expenseId}";

/// Everything the handlers need, built once.
#[derive(Debug, Clone)]
pub(crate) struct ExpensesApi {
    create: CreateExpense,
    get: GetExpense,
    update: UpdateExpense,
    delete: DeleteExpense,
    transition: TransitionExpense,
}

impl ExpensesApi {
    pub(crate) fn new(
        pool: &PgPool,
        clock: &Arc<dyn Clock>,
        memberships: &Arc<dyn MembershipLookup>,
        receipts: &Arc<dyn ReceiptChecks>,
    ) -> Self {
        Self {
            create: CreateExpense::new(pool.clone(), clock.clone(), memberships.clone()),
            get: GetExpense::new(pool.clone(), memberships.clone()),
            update: UpdateExpense::new(pool.clone(), clock.clone(), memberships.clone()),
            delete: DeleteExpense::new(pool.clone(), memberships.clone()),
            transition: TransitionExpense::new(pool.clone(), clock.clone(), memberships.clone(), receipts.clone()),
        }
    }

    /// The expense endpoints of M3. List, batch and export follow in M4 and M8.
    pub(crate) fn routes(self) -> Routes {
        let action = |action: Action, template: &'static str, routes: Routes, api: &Self| {
            let handler = move |state, user, parameters, request| transition(state, user, parameters, request, action);
            let body = if action == Action::Reject {
                Body::Json
            } else {
                Body::Ignored
            };
            routes.route(Method::POST, template, body, handler, api.clone())
        };
        let routes = Routes::new()
            .route(Method::POST, EXPENSES, Body::Json, create, self.clone())
            .route(Method::GET, EXPENSE, Body::Ignored, get, self.clone())
            .route(Method::PATCH, EXPENSE, Body::Json, update, self.clone())
            .route(Method::DELETE, EXPENSE, Body::Ignored, delete, self.clone());
        [
            (Action::Submit, "/api/v1/teams/{teamId}/expenses/{expenseId}:submit"),
            (Action::Withdraw, "/api/v1/teams/{teamId}/expenses/{expenseId}:withdraw"),
            (Action::Approve, "/api/v1/teams/{teamId}/expenses/{expenseId}:approve"),
            (Action::Reject, "/api/v1/teams/{teamId}/expenses/{expenseId}:reject"),
            (Action::Reopen, "/api/v1/teams/{teamId}/expenses/{expenseId}:reopen"),
            (
                Action::Reimburse,
                "/api/v1/teams/{teamId}/expenses/{expenseId}:reimburse",
            ),
        ]
        .into_iter()
        .fold(routes, |routes, (verb, template)| action(verb, template, routes, &self))
    }
}

/// `POST /teams/{teamId}/expenses`: 201 with `Location` and `ETag`.
async fn create(
    State(api): State<ExpensesApi>,
    user: CurrentUser,
    parameters: PathParameters,
    request: Request,
) -> Result<Response, Problem> {
    let team = team(&parameters)?;
    let headers = request.headers().clone();
    let body: CreateExpenseRequest = read_json(request).await?;
    let idempotency_key = header_text(&headers, "idempotency-key");
    let mut violations = Violations::default();
    violations.size("idempotencyKey", idempotency_key.as_deref(), 8, 128);
    violations.into_result()?;
    let money = body.money.as_ref().map(dto::MoneyBody::to_money).transpose()?;
    let (Some(money), Some(category), Some(incurred_on), Some(merchant)) =
        (money, body.category, body.incurred_on, body.merchant)
    else {
        // Validation has required all four.
        return Err(Problem::internal_error());
    };
    let command = CreateExpenseCommand {
        team,
        claimant: user.id(),
        expense: NewExpense {
            money,
            category: category.0,
            incurred_on: incurred_on.0,
            merchant,
            note: body.note,
        },
    };
    let created = api
        .create
        .execute(command, idempotency_key.as_deref())
        .await
        .map_err(|e| problem::failure(&e))?;
    let location = format!("/api/v1/teams/{team}/expenses/{}", created.value.id());
    let mut response = (StatusCode::CREATED, axum::Json(ExpenseResponse::from(&created))).into_response();
    insert_header(response.headers_mut(), header::LOCATION, &location);
    insert_header(response.headers_mut(), header::ETAG, &etag(created.version));
    Ok(response)
}

/// `GET /teams/{teamId}/expenses/{expenseId}`: 200 with `ETag`.
async fn get(
    State(api): State<ExpensesApi>,
    user: CurrentUser,
    parameters: PathParameters,
) -> Result<Response, Problem> {
    let (team, id) = (team(&parameters)?, expense(&parameters)?);
    let found = api
        .get
        .execute(team, id, user.id())
        .await
        .map_err(|e| problem::failure(&e))?;
    Ok(ok(&found))
}

/// `PATCH /teams/{teamId}/expenses/{expenseId}`: `If-Match` required.
async fn update(
    State(api): State<ExpensesApi>,
    user: CurrentUser,
    parameters: PathParameters,
    request: Request,
) -> Result<Response, Problem> {
    let if_match = required_if_match(request.headers())?;
    let (team, id) = (team(&parameters)?, expense(&parameters)?);
    let body: UpdateExpenseRequest = read_json(request).await?;
    check_if_match_length(&if_match)?;
    let expected = parse_version(&if_match)?;
    let money = body.money.as_ref().map(dto::MoneyBody::to_money).transpose()?;
    let command = UpdateExpenseCommand {
        team,
        id,
        caller: user.id(),
        expected,
        money,
        category: body.category.map(|category| category.0),
        incurred_on: body.incurred_on.map(|date| date.0),
        merchant: body.merchant,
        note: body.note,
    };
    let updated = api.update.execute(command).await.map_err(|e| problem::failure(&e))?;
    Ok(ok(&updated))
}

/// `DELETE /teams/{teamId}/expenses/{expenseId}`: 204, `If-Match` required.
async fn delete(
    State(api): State<ExpensesApi>,
    user: CurrentUser,
    parameters: PathParameters,
    headers: HeaderMap,
) -> Result<Response, Problem> {
    let if_match = required_if_match(&headers)?;
    let (team, id) = (team(&parameters)?, expense(&parameters)?);
    check_if_match_length(&if_match)?;
    let expected = parse_version(&if_match)?;
    api.delete
        .execute(team, id, user.id(), expected)
        .await
        .map_err(|e| problem::failure(&e))?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

/// `POST /teams/{teamId}/expenses/{expenseId}:<action>`: 200 with `ETag`. Only `:reject` reads a body.
async fn transition(
    State(api): State<ExpensesApi>,
    user: CurrentUser,
    parameters: PathParameters,
    request: Request,
    action: Action,
) -> Result<Response, Problem> {
    let (team, id) = (team(&parameters)?, expense(&parameters)?);
    let reason = if action == Action::Reject {
        let body: RejectRequest = read_json(request).await?;
        body.reason
    } else {
        None
    };
    let changed = api
        .transition
        .execute(action, team, id, user.id(), reason.as_deref())
        .await
        .map_err(|e| problem::failure(&e))?;
    Ok(ok(&changed))
}

fn team(parameters: &PathParameters) -> Result<TeamId, Problem> {
    uuid_parameter(parameters, "teamId").map(TeamId::from_uuid)
}

fn expense(parameters: &PathParameters) -> Result<ExpenseId, Problem> {
    uuid_parameter(parameters, "expenseId").map(ExpenseId::from_uuid)
}

/// 200 with the expense and its version as the ETag.
fn ok(expense: &Versioned<Expense>) -> Response {
    let mut response = axum::Json(ExpenseResponse::from(expense)).into_response();
    insert_header(response.headers_mut(), header::ETAG, &etag(expense.version));
    response
}

/// `"<version>"`, a strong ETag as Spring writes `ResponseEntity.eTag`.
fn etag(version: Version) -> String {
    format!("\"{version}\"")
}

fn insert_header(headers: &mut HeaderMap, name: header::HeaderName, value: &str) {
    if let Ok(value) = HeaderValue::from_str(value) {
        headers.insert(name, value);
    }
}

/// A missing `If-Match` is 428, the one missing header Java does not answer with 400.
fn required_if_match(headers: &HeaderMap) -> Result<String, Problem> {
    header_text(headers, "if-match").ok_or_else(|| Problem::precondition_required("If-Match"))
}

/// The spec's `maxLength: 64`, which Java's generated interface checks with `@Size(max = 64)` on the parameter
/// `ifMatch`.
fn check_if_match_length(if_match: &str) -> Result<(), Problem> {
    let mut violations = Violations::default();
    violations.size("ifMatch", Some(if_match), 0, 64);
    violations.into_result()
}

/// Accepts `"3"` and `W/"3"`. Anything else cannot match a version, so it counts as stale: 412.
/// Java: `ExpensesController.parseVersion`, with `Long.parseLong`.
fn parse_version(if_match: &str) -> Result<Version, Problem> {
    let mut value = strip(if_match);
    value = value.strip_prefix("W/").unwrap_or(value);
    if value.len() >= 2
        && let Some(unquoted) = value.strip_prefix('"').and_then(|rest| rest.strip_suffix('"'))
    {
        value = unquoted;
    }
    value
        .parse::<i64>()
        .map(Version::from_stored)
        .map_err(|_| Problem::precondition_failed())
}
