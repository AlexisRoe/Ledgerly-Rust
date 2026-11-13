//! The HTTP edge every module shares: the route table, and how a refused request is answered.
//!
//! Java counterpart: what Spring MVC does around the generated `*Api` interfaces (handler mapping, argument
//! resolution, `@RestControllerAdvice`). Here it is written by hand (ADR 0014).

mod routing;

pub use routing::{Body, PathParameters, RouteRejection, RouteService, Routes};
