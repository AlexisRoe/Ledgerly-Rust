//! The receipts endpoints under `/api/v1/teams/{teamId}/expenses/{expenseId}/receipts`.
//!
//! Java counterpart: `receipts/internal/web/ReceiptsController.java`. There is no role check in front of the
//! use cases, as in Java: each asks the expenses module whether the caller may see the expense, or change its
//! receipts.

mod dto;
#[cfg(test)]
pub(crate) mod tests;

use axum::Json;
use axum::extract::{Request, State};
use axum::http::{HeaderValue, Method, StatusCode, header};
use axum::response::{IntoResponse, Response};
use ledgerly_shared::id::{ExpenseId, ReceiptId, TeamId};
use ledgerly_shared::iso::format_instant;
use ledgerly_shared::web::{
    Body, CurrentUser, ErrorCode, MultipartUpload, PathParameters, Problem, Routes, accept, uuid_parameter,
};

use self::dto::{DownloadLink, ReceiptResponse};
use crate::application::{DeleteReceipt, DownloadReceipt, ListReceipts, UploadReceipt, UseCaseError};

const RECEIPTS: &str = "/api/v1/teams/{teamId}/expenses/{expenseId}/receipts";
const RECEIPT: &str = "/api/v1/teams/{teamId}/expenses/{expenseId}/receipts/{receiptId}";
const DOWNLOAD: &str = "/api/v1/teams/{teamId}/expenses/{expenseId}/receipts/{receiptId}/download";

/// The use cases behind the endpoints.
#[derive(Debug, Clone)]
pub(crate) struct ReceiptsApi {
    pub upload: UploadReceipt,
    pub list: ListReceipts,
    pub download: DownloadReceipt,
    pub delete: DeleteReceipt,
}

impl ReceiptsApi {
    pub(crate) fn routes(self) -> Routes {
        Routes::new()
            .route(Method::GET, RECEIPTS, Body::Ignored, list, self.clone())
            .route(Method::POST, RECEIPTS, Body::Multipart, upload, self.clone())
            .route(Method::DELETE, RECEIPT, Body::Ignored, delete, self.clone())
            .producing(accept::PROBLEM)
            .route(Method::GET, DOWNLOAD, Body::Ignored, download, self)
    }
}

/// `POST …/receipts`: 202 with the receipt in `PENDING_SCAN`, and `Location` pointing at the list, which is
/// where its scan status shows.
async fn upload(
    State(api): State<ReceiptsApi>,
    user: CurrentUser,
    parameters: PathParameters,
    request: Request,
) -> Result<Response, Problem> {
    let (team, expense) = expense(&parameters)?;
    let upload = MultipartUpload::new(request)?;
    let receipt = api
        .upload
        .execute(team, expense, user.id(), upload)
        .await
        .map_err(failure)?;
    let mut response = (StatusCode::ACCEPTED, Json(ReceiptResponse::from(receipt))).into_response();
    if let Ok(location) = HeaderValue::from_str(&format!("/api/v1/teams/{team}/expenses/{expense}/receipts")) {
        response.headers_mut().insert(header::LOCATION, location);
    }
    Ok(response)
}

/// `GET …/receipts`: every receipt of the expense, whatever its scan status.
async fn list(
    State(api): State<ReceiptsApi>,
    user: CurrentUser,
    parameters: PathParameters,
) -> Result<Json<Vec<ReceiptResponse>>, Problem> {
    let (team, expense) = expense(&parameters)?;
    let receipts = api.list.execute(team, expense, user.id()).await.map_err(failure)?;
    Ok(Json(receipts.into_iter().map(ReceiptResponse::from).collect()))
}

/// `GET …/receipts/{receiptId}/download`: a link valid for five minutes, never cached.
async fn download(
    State(api): State<ReceiptsApi>,
    user: CurrentUser,
    parameters: PathParameters,
) -> Result<Response, Problem> {
    let (team, expense) = expense(&parameters)?;
    let receipt = receipt(&parameters)?;
    let link = api
        .download
        .execute(team, expense, receipt, user.id())
        .await
        .map_err(failure)?;
    let body = DownloadLink {
        url: link.url,
        expires_at: format_instant(link.expires_at),
    };
    let mut response = Json(body).into_response();
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok(response)
}

/// `DELETE …/receipts/{receiptId}`: 204.
async fn delete(
    State(api): State<ReceiptsApi>,
    user: CurrentUser,
    parameters: PathParameters,
) -> Result<StatusCode, Problem> {
    let (team, expense) = expense(&parameters)?;
    let receipt = receipt(&parameters)?;
    api.delete
        .execute(team, expense, receipt, user.id())
        .await
        .map_err(failure)?;
    Ok(StatusCode::NO_CONTENT)
}

fn receipt(parameters: &PathParameters) -> Result<ReceiptId, Problem> {
    uuid_parameter(parameters, "receiptId").map(ReceiptId::from_uuid)
}

fn expense(parameters: &PathParameters) -> Result<(TeamId, ExpenseId), Problem> {
    let team = uuid_parameter(parameters, "teamId").map(TeamId::from_uuid)?;
    let expense = uuid_parameter(parameters, "expenseId").map(ExpenseId::from_uuid)?;
    Ok((team, expense))
}

/// Java's answer for each failure. Storage and database failures are logged by kind and answered with a bare
/// 500; neither the log nor the body names a key or a file.
fn failure(error: UseCaseError) -> Problem {
    match error {
        UseCaseError::Expense(refusal) => refusal.problem(),
        UseCaseError::Upload(problem) => problem,
        UseCaseError::NotFound => Problem::not_found(),
        UseCaseError::Empty => Problem::invalid_request("file", "The file is empty"),
        UseCaseError::TooLarge(megabytes) => {
            Problem::payload_too_large(format!("The file is larger than {megabytes} MB"))
        }
        UseCaseError::UnsupportedType => Problem::new(
            ErrorCode::UnsupportedMediaType,
            "Only PDF, JPEG and PNG files are accepted",
        ),
        UseCaseError::Duplicate => Problem::new(
            ErrorCode::ReceiptDuplicate,
            "This file is already attached to the expense",
        ),
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
