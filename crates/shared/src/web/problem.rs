//! RFC 9457 problem bodies with Ledgerly's extensions, and the catalogue of every error the API reports.
//!
//! Java counterpart: `shared/error/ErrorCatalogue.java`, `ProblemFactory.java` and the exception classes beside
//! them. Java throws an exception and lets a `@RestControllerAdvice` build the body; here a handler returns a
//! [`Problem`], which is a response like any other.
//!
//! A problem is rendered twice. [`Problem::into_response`] writes the body it can write on its own and keeps
//! the problem in the response's extensions; [`render`], a middleware that sees the request, writes it again
//! with `instance` (the request path) and `traceId` (from [`TraceContext`]). Java gets both from the servlet
//! request and the current span, which a Rust value has no hidden access to.

use axum::body::Body;
use axum::extract::Request;
use axum::http::{HeaderName, HeaderValue, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use serde::Serialize;

/// The ids of the request being handled, as lower-case hex: 32 characters and 16 characters.
///
/// The tracing middleware puts it into the request's extensions, so that a problem body reports the same
/// `traceId` the log lines carry. Java: the current Micrometer span.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraceContext {
    trace_id: String,
    span_id: String,
}

impl TraceContext {
    #[must_use]
    pub const fn new(trace_id: String, span_id: String) -> Self {
        Self { trace_id, span_id }
    }

    #[must_use]
    pub fn trace_id(&self) -> &str {
        &self.trace_id
    }

    #[must_use]
    pub fn span_id(&self) -> &str {
        &self.span_id
    }
}

/// Every error the API reports by name. The `code` is the variant's name in upper snake case, the `type` is
/// `https://ledgerly.dev/problems/<slug>`, and status and title are fixed per entry.
///
/// Java counterpart: `ErrorCatalogue`. Clients switch on the code, so neither the names nor the slugs may change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCode {
    ValidationFailed,
    MalformedRequest,
    Unauthenticated,
    Forbidden,
    NotFound,
    ExpenseInvalidTransition,
    ExpenseSelfDealing,
    LastOwner,
    AlreadyMember,
    InvitationNotPending,
    InvitationExpired,
    EmailNotVerified,
    ExpenseNotDraft,
    IdempotencyInProgress,
    PreconditionFailed,
    PreconditionRequired,
    ExpenseRuleViolated,
    IdempotencyKeyReused,
    PayloadTooLarge,
    UnsupportedMediaType,
    ReceiptDuplicate,
    RateLimited,
    BatchRejected,
    InternalError,
    RequestRejected,
}

impl ErrorCode {
    /// Every entry, in Java's declaration order.
    pub const ALL: [Self; 25] = [
        Self::ValidationFailed,
        Self::MalformedRequest,
        Self::Unauthenticated,
        Self::Forbidden,
        Self::NotFound,
        Self::ExpenseInvalidTransition,
        Self::ExpenseSelfDealing,
        Self::LastOwner,
        Self::AlreadyMember,
        Self::InvitationNotPending,
        Self::InvitationExpired,
        Self::EmailNotVerified,
        Self::ExpenseNotDraft,
        Self::IdempotencyInProgress,
        Self::PreconditionFailed,
        Self::PreconditionRequired,
        Self::ExpenseRuleViolated,
        Self::IdempotencyKeyReused,
        Self::PayloadTooLarge,
        Self::UnsupportedMediaType,
        Self::ReceiptDuplicate,
        Self::RateLimited,
        Self::BatchRejected,
        Self::InternalError,
        Self::RequestRejected,
    ];

    /// `(code, slug, title, status)`, one exhaustive `match` so that a new entry cannot go without any of them.
    const fn entry(self) -> (&'static str, &'static str, &'static str, StatusCode) {
        match self {
            Self::ValidationFailed => (
                "VALIDATION_FAILED",
                "validation-failed",
                "Request validation failed",
                StatusCode::BAD_REQUEST,
            ),
            Self::MalformedRequest => (
                "MALFORMED_REQUEST",
                "malformed-request",
                "Malformed request",
                StatusCode::BAD_REQUEST,
            ),
            Self::Unauthenticated => (
                "UNAUTHENTICATED",
                "unauthenticated",
                "Authentication required",
                StatusCode::UNAUTHORIZED,
            ),
            Self::Forbidden => (
                "FORBIDDEN",
                "forbidden",
                "Action not allowed for your role",
                StatusCode::FORBIDDEN,
            ),
            Self::NotFound => ("NOT_FOUND", "not-found", "Resource not found", StatusCode::NOT_FOUND),
            Self::ExpenseInvalidTransition => (
                "EXPENSE_INVALID_TRANSITION",
                "invalid-transition",
                "Illegal workflow transition",
                StatusCode::CONFLICT,
            ),
            Self::ExpenseSelfDealing => (
                "EXPENSE_SELF_DEALING",
                "self-dealing",
                "Self-dealing is not allowed",
                StatusCode::CONFLICT,
            ),
            Self::LastOwner => (
                "LAST_OWNER",
                "last-owner",
                "A team must keep at least one owner",
                StatusCode::CONFLICT,
            ),
            Self::AlreadyMember => (
                "ALREADY_MEMBER",
                "already-member",
                "Already a teammate",
                StatusCode::CONFLICT,
            ),
            Self::InvitationNotPending => (
                "INVITATION_NOT_PENDING",
                "invitation-not-pending",
                "Invitation is no longer pending",
                StatusCode::CONFLICT,
            ),
            Self::InvitationExpired => (
                "INVITATION_EXPIRED",
                "invitation-expired",
                "Invitation has expired",
                StatusCode::CONFLICT,
            ),
            Self::EmailNotVerified => (
                "EMAIL_NOT_VERIFIED",
                "email-not-verified",
                "Verify your email address first",
                StatusCode::FORBIDDEN,
            ),
            Self::ExpenseNotDraft => (
                "EXPENSE_NOT_DRAFT",
                "not-draft",
                "Only a draft allows this",
                StatusCode::CONFLICT,
            ),
            Self::IdempotencyInProgress => (
                "IDEMPOTENCY_IN_PROGRESS",
                "idempotency-in-progress",
                "Request already in progress",
                StatusCode::CONFLICT,
            ),
            Self::PreconditionFailed => (
                "PRECONDITION_FAILED",
                "precondition-failed",
                "ETag does not match",
                StatusCode::PRECONDITION_FAILED,
            ),
            Self::PreconditionRequired => (
                "PRECONDITION_REQUIRED",
                "precondition-required",
                "If-Match header required",
                StatusCode::PRECONDITION_REQUIRED,
            ),
            Self::ExpenseRuleViolated => (
                "EXPENSE_RULE_VIOLATED",
                "rule-violated",
                "Business rule violated",
                StatusCode::UNPROCESSABLE_ENTITY,
            ),
            Self::IdempotencyKeyReused => (
                "IDEMPOTENCY_KEY_REUSED",
                "idempotency-key-reused",
                "Idempotency key reused with a different body",
                StatusCode::UNPROCESSABLE_ENTITY,
            ),
            Self::PayloadTooLarge => (
                "PAYLOAD_TOO_LARGE",
                "payload-too-large",
                "Request body too large",
                StatusCode::PAYLOAD_TOO_LARGE,
            ),
            Self::UnsupportedMediaType => (
                "UNSUPPORTED_MEDIA_TYPE",
                "unsupported-media-type",
                "Unsupported file type",
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
            ),
            Self::ReceiptDuplicate => (
                "RECEIPT_DUPLICATE",
                "receipt-duplicate",
                "This file is already attached to the expense",
                StatusCode::CONFLICT,
            ),
            Self::RateLimited => (
                "RATE_LIMITED",
                "rate-limited",
                "Too many requests",
                StatusCode::TOO_MANY_REQUESTS,
            ),
            Self::BatchRejected => (
                "BATCH_REJECTED",
                "batch-rejected",
                "The batch was rejected as a whole",
                StatusCode::UNPROCESSABLE_ENTITY,
            ),
            Self::InternalError => (
                "INTERNAL_ERROR",
                "internal-error",
                "Unexpected error",
                StatusCode::INTERNAL_SERVER_ERROR,
            ),
            Self::RequestRejected => (
                "REQUEST_REJECTED",
                "request-rejected",
                "Request rejected",
                StatusCode::BAD_REQUEST,
            ),
        }
    }

    #[must_use]
    pub const fn code(self) -> &'static str {
        self.entry().0
    }

    /// The `type` URI.
    #[must_use]
    pub fn type_uri(self) -> String {
        format!("https://ledgerly.dev/problems/{}", self.entry().1)
    }

    #[must_use]
    pub const fn title(self) -> &'static str {
        self.entry().2
    }

    #[must_use]
    pub const fn status(self) -> StatusCode {
        self.entry().3
    }
}

/// One invalid field: its name as the API spells it, and what is wrong with it. Java: `FieldViolation`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FieldViolation {
    pub field: String,
    pub message: String,
}

impl FieldViolation {
    #[must_use]
    pub fn new(field: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            field: field.into(),
            message: message.into(),
        }
    }
}

/// An error response. Return it from a handler, or turn it into a response with `into_response`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    status: StatusCode,
    kind: Kind,
    detail: Option<String>,
    violations: Vec<FieldViolation>,
    headers: Vec<(HeaderName, HeaderValue)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Kind {
    /// One of ours: `type` and `title` from the catalogue.
    Catalogued(ErrorCode),
    /// One Spring built itself and `ProblemFactory.enrich` only added `code: REQUEST_REJECTED` to: `type` stays
    /// `about:blank` and `title` is the status's reason phrase.
    Framework,
}

/// `VALIDATION_FAILED`'s fixed detail.
const VALIDATION_DETAIL: &str = "Request validation failed";

impl Problem {
    /// A catalogue entry with its status, and a detail for the client. Java: the `LedgerlyException` subclasses.
    ///
    /// The detail must never carry secrets or personal data.
    #[must_use]
    pub fn new(code: ErrorCode, detail: impl Into<String>) -> Self {
        Self::of(code, Some(detail.into()), Vec::new())
    }

    fn of(code: ErrorCode, detail: Option<String>, violations: Vec<FieldViolation>) -> Self {
        Self {
            status: code.status(),
            kind: Kind::Catalogued(code),
            detail,
            violations,
            headers: Vec::new(),
        }
    }

    /// 400 `VALIDATION_FAILED` with the given fields. Java: `MethodArgumentNotValidException` and friends.
    #[must_use]
    pub fn validation_failed(violations: Vec<FieldViolation>) -> Self {
        Self::of(
            ErrorCode::ValidationFailed,
            Some(VALIDATION_DETAIL.to_owned()),
            violations,
        )
    }

    /// 400 `VALIDATION_FAILED` for one field. Java: `InvalidRequest`.
    #[must_use]
    pub fn invalid_request(field: impl Into<String>, message: impl Into<String>) -> Self {
        Self::validation_failed(vec![FieldViolation::new(field, message)])
    }

    /// 400 `MALFORMED_REQUEST`: the body is missing, is not JSON, or does not fit the request type.
    #[must_use]
    pub fn malformed_request() -> Self {
        Self::new(
            ErrorCode::MalformedRequest,
            "The request body is missing, malformed or contains unknown fields",
        )
    }

    /// 401, with the `WWW-Authenticate: Bearer` challenge. Java: `ProblemSecurityHandler.commence`.
    #[must_use]
    pub fn unauthenticated() -> Self {
        Self::new(ErrorCode::Unauthenticated, "Authentication required")
            .with_header(header::WWW_AUTHENTICATE, HeaderValue::from_static("Bearer"))
    }

    /// 403 `FORBIDDEN`. Java: `Forbidden`.
    #[must_use]
    pub fn forbidden(detail: impl Into<String>) -> Self {
        Self::new(ErrorCode::Forbidden, detail)
    }

    /// 404 `NOT_FOUND`, the same for "does not exist" and "you may not know it exists". Java: `NotFound`.
    #[must_use]
    pub fn not_found() -> Self {
        Self::new(ErrorCode::NotFound, "Resource not found")
    }

    /// 412: the `If-Match` version is not the current one. Java: `PreconditionFailed`.
    #[must_use]
    pub fn precondition_failed() -> Self {
        Self::new(ErrorCode::PreconditionFailed, "The resource changed since you read it")
    }

    /// 428: a conditional header is missing. Java: `PreconditionRequired`.
    #[must_use]
    pub fn precondition_required(header: &str) -> Self {
        Self::new(
            ErrorCode::PreconditionRequired,
            format!("The {header} header is required"),
        )
    }

    /// 413 `PAYLOAD_TOO_LARGE`. Java: `PayloadTooLarge`.
    #[must_use]
    pub fn payload_too_large(detail: impl Into<String>) -> Self {
        Self::new(ErrorCode::PayloadTooLarge, detail)
    }

    /// 500 without a detail: nothing about the cause leaves the server, only the `traceId` to find it in the log.
    #[must_use]
    pub fn internal_error() -> Self {
        Self::of(ErrorCode::InternalError, None, Vec::new())
    }

    /// A rejection Spring produced itself (unknown path, wrong method, wrong media type, unconvertible path
    /// parameter), with `code: REQUEST_REJECTED` added as Java's `ProblemFactory.enrich` does.
    #[must_use]
    pub fn rejected(status: StatusCode, detail: impl Into<String>) -> Self {
        Self {
            status,
            kind: Kind::Framework,
            detail: Some(detail.into()),
            violations: Vec::new(),
            headers: Vec::new(),
        }
    }

    /// The same problem with an extra response header, such as `Allow` or `Retry-After`.
    #[must_use]
    pub fn with_header(mut self, name: HeaderName, value: HeaderValue) -> Self {
        self.headers.push((name, value));
        self
    }

    #[must_use]
    pub const fn status(&self) -> StatusCode {
        self.status
    }

    /// The catalogue code the body carries.
    #[must_use]
    pub const fn code(&self) -> ErrorCode {
        match self.kind {
            Kind::Catalogued(code) => code,
            Kind::Framework => ErrorCode::RequestRejected,
        }
    }

    #[must_use]
    pub fn detail(&self) -> Option<&str> {
        self.detail.as_deref()
    }

    #[must_use]
    pub fn violations(&self) -> &[FieldViolation] {
        &self.violations
    }

    /// The body, in the member order Spring writes a `ProblemDetail`.
    fn body(&self, instance: Option<&str>, trace_id: Option<&str>) -> Body {
        let (type_uri, title) = match self.kind {
            Kind::Catalogued(code) => (code.type_uri(), code.title()),
            Kind::Framework => (
                "about:blank".to_owned(),
                self.status.canonical_reason().unwrap_or_default(),
            ),
        };
        let json = ProblemJson {
            type_uri,
            title,
            status: self.status.as_u16(),
            detail: self.detail.as_deref(),
            instance,
            code: self.code().code(),
            trace_id,
            errors: (!self.violations.is_empty()).then_some(self.violations.as_slice()),
        };
        // Serialising strings, numbers and a list of string pairs cannot fail; the fallback is never used.
        Body::from(serde_json::to_vec(&json).unwrap_or_default())
    }
}

#[derive(Serialize)]
struct ProblemJson<'a> {
    #[serde(rename = "type")]
    type_uri: String,
    title: &'a str,
    status: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    instance: Option<&'a str>,
    code: &'a str,
    #[serde(rename = "traceId", skip_serializing_if = "Option::is_none")]
    trace_id: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    errors: Option<&'a [FieldViolation]>,
}

const PROBLEM_JSON: HeaderValue = HeaderValue::from_static("application/problem+json");

impl IntoResponse for Problem {
    fn into_response(self) -> Response {
        let mut response = Response::new(self.body(None, None));
        *response.status_mut() = self.status;
        let headers = response.headers_mut();
        headers.insert(header::CONTENT_TYPE, PROBLEM_JSON);
        for (name, value) in &self.headers {
            headers.append(name.clone(), value.clone());
        }
        response.extensions_mut().insert(self);
        response
    }
}

/// Middleware: writes every problem body again with `instance` (the request path) and `traceId`.
///
/// It must run inside the tracing middleware, which provides the [`TraceContext`], and outside everything that
/// returns problems.
pub async fn render(request: Request, next: Next) -> Response {
    let instance = request.uri().path().to_owned();
    let trace_id = request
        .extensions()
        .get::<TraceContext>()
        .map(|context| context.trace_id().to_owned());
    let mut response = next.run(request).await;
    if let Some(problem) = response.extensions_mut().remove::<Problem>() {
        let body = problem.body(Some(&instance), trace_id.as_deref());
        response.headers_mut().remove(header::CONTENT_LENGTH);
        *response.body_mut() = body;
    }
    response
}

#[cfg(test)]
mod tests {
    use axum::Router;
    use axum::http::HeaderMap;
    use axum::routing::get;
    use serde_json::{Value, json};
    use tower::ServiceExt;

    use super::*;

    const TRACE: &str = "0af7651916cd43dd8448eb211c80319c";

    /// Serves `problem` at `/api/v1/x` behind the render middleware, with a known trace id.
    async fn served(problem: Problem) -> (StatusCode, HeaderMap, Value) {
        let router = Router::new()
            .route("/api/v1/x", get(move || std::future::ready(problem)))
            .layer(axum::middleware::from_fn(render))
            .layer(axum::middleware::from_fn(
                |mut request: Request, next: Next| async move {
                    request
                        .extensions_mut()
                        .insert(TraceContext::new(TRACE.to_owned(), "b7ad6b7169203331".to_owned()));
                    next.run(request).await
                },
            ));
        let response = router
            .oneshot(axum::http::Request::get("/api/v1/x?q=1").body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let headers = response.headers().clone();
        let body = axum::body::to_bytes(response.into_body(), 4096).await.unwrap();
        (status, headers, serde_json::from_slice(&body).unwrap())
    }

    /// Java's shape for each entry: what `ProblemFactory.create(error, detail, violations)` writes, with the
    /// `instance` Spring adds.
    #[tokio::test]
    async fn every_catalogue_entry_renders_javas_full_body() {
        let expected: [(&str, u16, &str, &str); 25] = [
            (
                "VALIDATION_FAILED",
                400,
                "validation-failed",
                "Request validation failed",
            ),
            ("MALFORMED_REQUEST", 400, "malformed-request", "Malformed request"),
            ("UNAUTHENTICATED", 401, "unauthenticated", "Authentication required"),
            ("FORBIDDEN", 403, "forbidden", "Action not allowed for your role"),
            ("NOT_FOUND", 404, "not-found", "Resource not found"),
            (
                "EXPENSE_INVALID_TRANSITION",
                409,
                "invalid-transition",
                "Illegal workflow transition",
            ),
            (
                "EXPENSE_SELF_DEALING",
                409,
                "self-dealing",
                "Self-dealing is not allowed",
            ),
            ("LAST_OWNER", 409, "last-owner", "A team must keep at least one owner"),
            ("ALREADY_MEMBER", 409, "already-member", "Already a teammate"),
            (
                "INVITATION_NOT_PENDING",
                409,
                "invitation-not-pending",
                "Invitation is no longer pending",
            ),
            (
                "INVITATION_EXPIRED",
                409,
                "invitation-expired",
                "Invitation has expired",
            ),
            (
                "EMAIL_NOT_VERIFIED",
                403,
                "email-not-verified",
                "Verify your email address first",
            ),
            ("EXPENSE_NOT_DRAFT", 409, "not-draft", "Only a draft allows this"),
            (
                "IDEMPOTENCY_IN_PROGRESS",
                409,
                "idempotency-in-progress",
                "Request already in progress",
            ),
            ("PRECONDITION_FAILED", 412, "precondition-failed", "ETag does not match"),
            (
                "PRECONDITION_REQUIRED",
                428,
                "precondition-required",
                "If-Match header required",
            ),
            ("EXPENSE_RULE_VIOLATED", 422, "rule-violated", "Business rule violated"),
            (
                "IDEMPOTENCY_KEY_REUSED",
                422,
                "idempotency-key-reused",
                "Idempotency key reused with a different body",
            ),
            ("PAYLOAD_TOO_LARGE", 413, "payload-too-large", "Request body too large"),
            (
                "UNSUPPORTED_MEDIA_TYPE",
                415,
                "unsupported-media-type",
                "Unsupported file type",
            ),
            (
                "RECEIPT_DUPLICATE",
                409,
                "receipt-duplicate",
                "This file is already attached to the expense",
            ),
            ("RATE_LIMITED", 429, "rate-limited", "Too many requests"),
            (
                "BATCH_REJECTED",
                422,
                "batch-rejected",
                "The batch was rejected as a whole",
            ),
            ("INTERNAL_ERROR", 500, "internal-error", "Unexpected error"),
            ("REQUEST_REJECTED", 400, "request-rejected", "Request rejected"),
        ];
        for (code, (name, status, slug, title)) in ErrorCode::ALL.into_iter().zip(expected) {
            let (served_status, headers, body) = served(Problem::new(code, "Some detail")).await;

            assert_eq!(served_status.as_u16(), status, "{name}");
            assert_eq!(headers[header::CONTENT_TYPE], "application/problem+json", "{name}");
            assert_eq!(
                body,
                json!({
                    "type": format!("https://ledgerly.dev/problems/{slug}"),
                    "title": title,
                    "status": status,
                    "detail": "Some detail",
                    "instance": "/api/v1/x",
                    "code": name,
                    "traceId": TRACE,
                }),
                "{name}"
            );
        }
    }

    #[tokio::test]
    async fn the_members_are_written_in_springs_order() {
        let router = Router::new()
            .route(
                "/x",
                get(|| async { Problem::invalid_request("money", "Not a valid decimal number") }),
            )
            .layer(axum::middleware::from_fn(render));
        let response = router
            .oneshot(axum::http::Request::get("/x").body(Body::empty()).unwrap())
            .await
            .unwrap();
        let body = axum::body::to_bytes(response.into_body(), 4096).await.unwrap();

        assert_eq!(
            std::str::from_utf8(&body).unwrap(),
            r#"{"type":"https://ledgerly.dev/problems/validation-failed","title":"Request validation failed","status":400,"detail":"Request validation failed","instance":"/x","code":"VALIDATION_FAILED","errors":[{"field":"money","message":"Not a valid decimal number"}]}"#,
            "no traceId without a trace context"
        );
    }

    #[tokio::test]
    async fn a_500_carries_nothing_but_the_trace_id() {
        let (status, _, body) = served(Problem::internal_error()).await;

        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(
            body,
            json!({
                "type": "https://ledgerly.dev/problems/internal-error",
                "title": "Unexpected error",
                "status": 500,
                "instance": "/api/v1/x",
                "code": "INTERNAL_ERROR",
                "traceId": TRACE,
            })
        );
    }

    #[tokio::test]
    async fn a_problem_spring_made_keeps_its_type_and_title_and_gets_request_rejected() {
        let problem = Problem::rejected(StatusCode::METHOD_NOT_ALLOWED, "Method 'PUT' is not supported.")
            .with_header(header::ALLOW, HeaderValue::from_static("GET"));
        let (status, headers, body) = served(problem).await;

        assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);
        assert_eq!(headers[header::ALLOW], "GET");
        assert_eq!(
            body,
            json!({
                "type": "about:blank",
                "title": "Method Not Allowed",
                "status": 405,
                "detail": "Method 'PUT' is not supported.",
                "instance": "/api/v1/x",
                "code": "REQUEST_REJECTED",
                "traceId": TRACE,
            })
        );
    }

    #[tokio::test]
    async fn the_fixed_details_are_javas() {
        let cases = [
            (Problem::not_found(), "Resource not found"),
            (Problem::unauthenticated(), "Authentication required"),
            (Problem::precondition_failed(), "The resource changed since you read it"),
            (
                Problem::precondition_required("If-Match"),
                "The If-Match header is required",
            ),
            (
                Problem::malformed_request(),
                "The request body is missing, malformed or contains unknown fields",
            ),
            (Problem::validation_failed(Vec::new()), "Request validation failed"),
        ];
        for (problem, detail) in cases {
            assert_eq!(problem.detail(), Some(detail));
        }
        let challenge = Problem::unauthenticated().into_response();
        assert_eq!(challenge.headers()[header::WWW_AUTHENTICATE], "Bearer");
    }
}
