//! The imports endpoints. Java counterpart: `imports/internal/web/ImportsController.java`.

#[cfg(test)]
mod tests;

use axum::Json;
use axum::extract::{Request, State};
use axum::http::{HeaderValue, Method, StatusCode, header};
use axum::response::{IntoResponse, Response};
use ledgerly_shared::id::{ImportId, TeamId};
use ledgerly_shared::web::{
    Body, CurrentUser, ErrorCode, MultipartUpload, PathParameters, Problem, Routes, uuid_parameter,
};
use serde::Serialize;

use crate::application::{GetImport, ImportView, UploadImport, UseCaseError};
use crate::domain::Refusal;

/// The use cases behind the endpoints.
#[derive(Debug, Clone)]
pub(crate) struct ImportsApi {
    pub upload: UploadImport,
    pub get: GetImport,
}

impl ImportsApi {
    pub(crate) fn routes(self) -> Routes {
        Routes::new()
            .route(
                Method::POST,
                "/api/v1/teams/{teamId}/imports",
                Body::Multipart,
                upload,
                self.clone(),
            )
            .route(
                Method::GET,
                "/api/v1/teams/{teamId}/imports/{importId}",
                Body::Ignored,
                get,
                self,
            )
    }
}

/// `POST /teams/{teamId}/imports`: 202 with `Location` and the queued import.
async fn upload(
    State(api): State<ImportsApi>,
    user: CurrentUser,
    parameters: PathParameters,
    request: Request,
) -> Result<Response, Problem> {
    let team = team(&parameters)?;
    let upload = MultipartUpload::new(request)?;
    let id = api.upload.execute(team, user.id(), upload).await.map_err(failure)?;
    let view = api.get.execute(team, id, user.id()).await.map_err(failure)?;
    let mut response = (StatusCode::ACCEPTED, Json(ImportResponse::from(view))).into_response();
    if let Ok(location) = HeaderValue::from_str(&format!("/api/v1/teams/{team}/imports/{id}")) {
        response.headers_mut().insert(header::LOCATION, location);
    }
    Ok(response)
}

/// `GET /teams/{teamId}/imports/{importId}`: the starter and OWNERs only.
async fn get(
    State(api): State<ImportsApi>,
    user: CurrentUser,
    parameters: PathParameters,
) -> Result<Json<ImportResponse>, Problem> {
    let team = team(&parameters)?;
    let id = uuid_parameter(&parameters, "importId").map(ImportId::from_uuid)?;
    let view = api.get.execute(team, id, user.id()).await.map_err(failure)?;
    Ok(Json(ImportResponse::from(view)))
}

fn team(parameters: &PathParameters) -> Result<TeamId, Problem> {
    uuid_parameter(parameters, "teamId").map(TeamId::from_uuid)
}

/// `ImportResponse`, in the spec's order.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ImportResponse {
    id: ImportId,
    status: &'static str,
    total_rows: i32,
    imported_rows: i32,
    skipped_rows: i64,
    errors: Vec<ImportRowError>,
    errors_truncated: bool,
}

/// `ImportRowError`.
#[derive(Debug, Serialize)]
struct ImportRowError {
    line: i32,
    code: String,
    message: String,
}

impl From<ImportView> for ImportResponse {
    fn from(view: ImportView) -> Self {
        Self {
            id: view.id,
            status: view.status.as_str(),
            total_rows: view.total_rows,
            imported_rows: view.imported_rows,
            skipped_rows: view.skipped_rows,
            errors: view
                .errors
                .into_iter()
                .map(|error| ImportRowError {
                    line: error.line,
                    code: error.code,
                    message: error.message,
                })
                .collect(),
            errors_truncated: view.errors_truncated,
        }
    }
}

/// Java's answer for each failure. Storage and database failures are logged by kind, answered with a bare 500.
fn failure(error: UseCaseError) -> Problem {
    match error {
        UseCaseError::NotFound => Problem::not_found(),
        UseCaseError::DailyLimit {
            limit,
            retry_after_seconds,
        } => {
            let problem = Problem::new(
                ErrorCode::RateLimited,
                format!("At most {limit} imports per team per day"),
            );
            match HeaderValue::from_str(&retry_after_seconds.to_string()) {
                Ok(value) => problem.with_header(header::RETRY_AFTER, value),
                Err(_) => problem,
            }
        }
        UseCaseError::Upload(problem) => problem,
        UseCaseError::TooLarge(megabytes) => {
            Problem::payload_too_large(format!("The file is larger than {megabytes} MB"))
        }
        UseCaseError::Refused(Refusal::Invalid(message)) => Problem::invalid_request("file", message),
        UseCaseError::Refused(Refusal::Unsupported(message)) => Problem::new(ErrorCode::UnsupportedMediaType, message),
        UseCaseError::Staging(_) => unhandled("staging"),
        UseCaseError::Storage(error) => {
            tracing::error!(error = %error, "Unhandled exception");
            Problem::internal_error()
        }
        UseCaseError::Database(_) => unhandled("database"),
    }
}

fn unhandled(kind: &'static str) -> Problem {
    tracing::error!(error = kind, "Unhandled exception");
    Problem::internal_error()
}
