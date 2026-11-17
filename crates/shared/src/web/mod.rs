//! The HTTP edge every module shares: the route table, reading requests, and problem responses.
//!
//! Java counterpart: what Spring MVC does around the generated `*Api` interfaces (handler mapping, argument
//! resolution, `@RestControllerAdvice`). Here it is written by hand (ADR 0014).

pub mod accept;
mod authentication;
mod body_limit;
mod browser;
mod current_user;
mod extract;
mod forwarded;
mod multipart;
mod problem;
mod query;
mod rate_limit;
mod routing;

pub use authentication::{Authentication, Checking, TokenCheck, TokenVerifier, authenticate};
pub use body_limit::{JsonBodyLimit, limit_json_bodies};
pub use browser::{Cors, cors, management_headers, secure_headers};
pub use current_user::CurrentUser;
pub(crate) use extract::parse_uuid;
pub use extract::{Json, MAX_JSON_BODY_SIZE, Validate, Violations, header_text, read_json, uuid_parameter};
pub use multipart::{MAX_FILE_SIZE, MAX_REQUEST_SIZE, MultipartUpload, UploadedFile};
pub use problem::{ErrorCode, FieldViolation, Problem, TraceContext, render};
pub use query::QueryParameters;
pub use rate_limit::{RequestLimits, limit_by_client_address, limit_by_user};
pub use routing::{Body, MatchedRoute, PathParameters, RouteRejection, RouteService, Routes};
