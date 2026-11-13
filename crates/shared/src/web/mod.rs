//! The HTTP edge every module shares: the route table, reading requests, and problem responses.
//!
//! Java counterpart: what Spring MVC does around the generated `*Api` interfaces (handler mapping, argument
//! resolution, `@RestControllerAdvice`). Here it is written by hand (ADR 0014).

mod extract;
mod problem;
mod routing;

pub use extract::{Json, MAX_JSON_BODY_SIZE, Validate, Violations, header_text, uuid_parameter};
pub use problem::{ErrorCode, FieldViolation, Problem, TraceContext, render};
pub use routing::{Body, PathParameters, RouteRejection, RouteService, Routes};
