//! The HTTP edge every module shares: the route table, reading requests, and problem responses.
//!
//! Java counterpart: what Spring MVC does around the generated `*Api` interfaces (handler mapping, argument
//! resolution, `@RestControllerAdvice`). Here it is written by hand (ADR 0014).

mod current_user;
mod extract;
mod problem;
mod query;
mod routing;

pub use current_user::CurrentUser;
pub use extract::{Json, MAX_JSON_BODY_SIZE, Validate, Violations, header_text, read_json, uuid_parameter};
pub use problem::{ErrorCode, FieldViolation, Problem, TraceContext, render};
pub use query::QueryParameters;
pub use routing::{Body, PathParameters, RouteRejection, RouteService, Routes};
