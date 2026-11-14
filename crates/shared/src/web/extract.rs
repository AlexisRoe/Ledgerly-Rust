//! Reading a request the way Spring's argument resolvers do, with Java's rejections.
//!
//! Java counterpart: `@RequestBody` with Jackson (`fail-on-unknown-properties: true`) and `@Valid`, `@PathVariable`
//! conversion to `UUID`, `@RequestHeader`, and the handlers in `shared/error/ProblemExceptionHandler.java` that
//! turn their exceptions into problems.
//!
//! Spring resolves a controller method's arguments in declaration order and stops at the first failure, so the
//! order in which a handler calls these decides which problem a client gets. Java's generated interfaces declare
//! headers and path variables before the body.

use axum::body::Bytes;
use axum::extract::{FromRequest, Request};
use axum::http::{HeaderMap, StatusCode};
use http_body_util::{BodyExt, LengthLimitError, Limited};
use serde::de::DeserializeOwned;
use uuid::Uuid;

use super::problem::{FieldViolation, Problem};
use super::routing::PathParameters;
use crate::text::utf16_len;

/// Java's `ledgerly.limits.max-json-body-size` default: 1 MiB.
///
/// Java enforces it in a filter that runs before authentication (`shared/web/JsonBodyLimitFilter.java`); that
/// filter, configurable and for every JSON request, arrives with the batch endpoint (M8). Until then the body
/// extractor stops reading at this size.
pub const MAX_JSON_BODY_SIZE: usize = 1024 * 1024;

/// A JSON body, parsed and then checked like Spring's `@Valid @RequestBody`.
///
/// - The body cannot be read, is not JSON, has a property `T` does not know, or a value of the wrong type:
///   400 `MALFORMED_REQUEST`. Duplicate properties are not an error; the last one wins, as in Jackson.
/// - Otherwise every rule of [`Validate`] is checked, and all broken ones are reported at once: 400
///   `VALIDATION_FAILED`.
///
/// `T` should have every property optional (`Option`), so that a missing required property reaches
/// [`Validate`] and is reported with all the others, as Bean Validation's `@NotNull` does, instead of failing the
/// parse at the first one.
#[derive(Debug, Clone)]
pub struct Json<T>(pub T);

/// The Bean Validation constraints of a request type: what Java's generated model declares with `@NotNull`,
/// `@Size` and `@Pattern`.
pub trait Validate {
    /// Adds a [`FieldViolation`] for every broken rule.
    fn validate(&self, violations: &mut Violations);
}

impl<T, S> FromRequest<S> for Json<T>
where
    T: DeserializeOwned + Validate,
    S: Send + Sync,
{
    type Rejection = Problem;

    async fn from_request(request: Request, _state: &S) -> Result<Self, Problem> {
        read_json(request).await.map(Self)
    }
}

/// What the [`Json`] extractor does, as a function: for a handler that must check its path parameters before
/// it reads the body, as Spring resolves them in that order.
///
/// # Errors
///
/// 413, `MALFORMED_REQUEST` or `VALIDATION_FAILED`, as described on [`Json`].
pub async fn read_json<T: DeserializeOwned + Validate>(request: Request) -> Result<T, Problem> {
    let bytes = read_body(request, MAX_JSON_BODY_SIZE).await?;
    let value: serde_json::Value = serde_json::from_slice(&bytes).map_err(|_| Problem::malformed_request())?;
    let body: T = serde_json::from_value(value).map_err(|_| Problem::malformed_request())?;
    let mut violations = Violations::default();
    body.validate(&mut violations);
    violations.into_result()?;
    Ok(body)
}

/// The whole body, or 413 once it grows past `limit` bytes.
async fn read_body(request: Request, limit: usize) -> Result<Bytes, Problem> {
    let too_large = || Problem::payload_too_large(format!("The request body is larger than {limit} bytes"));
    let declared = request
        .headers()
        .get(axum::http::header::CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok());
    if declared.is_some_and(|length| length > u64::try_from(limit).unwrap_or(u64::MAX)) {
        return Err(too_large());
    }
    match Limited::new(request.into_body(), limit).collect().await {
        Ok(collected) => Ok(collected.to_bytes()),
        Err(error) if error.is::<LengthLimitError>() => Err(too_large()),
        Err(_) => Err(Problem::malformed_request()),
    }
}

/// The broken rules of one request, in the order they were checked.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Violations(Vec<FieldViolation>);

impl Violations {
    /// `@NotNull`: the property is missing or `null`.
    pub fn require<T: ?Sized>(&mut self, field: &str, value: Option<&T>) {
        if value.is_none() {
            self.add(field, "must not be null");
        }
    }

    /// `@Size(min, max)` on a string, counted in UTF-16 code units as Java's `String.length()` does. A missing
    /// value is not checked; that is `@NotNull`'s job.
    pub fn size(&mut self, field: &str, value: Option<&str>, min: usize, max: usize) {
        if let Some(value) = value {
            let length = utf16_len(value);
            if length < min || length > max {
                self.add(field, format!("size must be between {min} and {max}"));
            }
        }
    }

    /// `@Pattern(regexp)`, given the check as a function: the message quotes `regexp` as Java's does.
    pub fn pattern(&mut self, field: &str, value: Option<&str>, regexp: &str, matches: impl Fn(&str) -> bool) {
        if let Some(value) = value
            && !matches(value)
        {
            self.add(field, format!("must match \"{regexp}\""));
        }
    }

    /// `@Email`, Hibernate's check (see [`crate::email`]), except that the empty text is refused too: Hibernate
    /// accepts `""`, so Java registers an account without an email (refused here by decision of the reviewer
    /// after M5). A missing value is not checked.
    pub fn email(&mut self, field: &str, value: Option<&str>) {
        if let Some(value) = value
            && (value.is_empty() || !crate::email::is_well_formed(value))
        {
            self.add(field, "must be a well-formed email address");
        }
    }

    /// Any other broken rule.
    pub fn add(&mut self, field: &str, message: impl Into<String>) {
        self.0.push(FieldViolation::new(field, message));
    }

    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// `Ok` if nothing is broken, else 400 `VALIDATION_FAILED` listing everything.
    ///
    /// # Errors
    ///
    /// The problem, if any rule is broken.
    pub fn into_result(self) -> Result<(), Problem> {
        if self.0.is_empty() {
            Ok(())
        } else {
            Err(Problem::validation_failed(self.0))
        }
    }
}

/// The path parameter `name` as a UUID. A value that is not one is Spring's type-mismatch 400, with
/// `REQUEST_REJECTED`; a parameter the template does not have is a programming error and a 500.
///
/// # Errors
///
/// The problem to answer with.
pub fn uuid_parameter(parameters: &PathParameters, name: &str) -> Result<Uuid, Problem> {
    let value = parameters.get(name).ok_or_else(Problem::internal_error)?;
    parse_uuid(value).ok_or_else(|| {
        Problem::rejected(
            StatusCode::BAD_REQUEST,
            format!("Failed to convert '{name}' with value: '{value}'"),
        )
    })
}

/// Only the canonical 8-4-4-4-12 form, in either case, which is what Java's `UUID.fromString` is given in
/// practice. (The `uuid` crate would also take braces, a `urn:` prefix or no hyphens; Java takes none of them.)
pub(crate) fn parse_uuid(value: &str) -> Option<Uuid> {
    let canonical = value.len() == 36
        && value
            .char_indices()
            .all(|(index, c)| matches!(index, 8 | 13 | 18 | 23) == (c == '-'));
    canonical.then(|| Uuid::try_parse(value).ok()).flatten()
}

/// The value of a request header as Java reads it: bytes as ISO-8859-1 characters. `None` if it is absent.
#[must_use]
pub fn header_text(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .map(|value| value.as_bytes().iter().map(|&byte| char::from(byte)).collect())
}

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use axum::response::IntoResponse;
    use serde::Deserialize;
    use serde_json::{Value, json};

    use super::*;

    #[derive(Debug, Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Lunch {
        merchant: Option<String>,
        note: Option<String>,
        count: Option<u32>,
    }

    impl Validate for Lunch {
        fn validate(&self, violations: &mut Violations) {
            violations.require("merchant", self.merchant.as_ref());
            violations.size("merchant", self.merchant.as_deref(), 1, 5);
            violations.size("note", self.note.as_deref(), 0, 3);
            violations.require("count", self.count.as_ref());
        }
    }

    async fn parse(body: impl Into<Body>, content_length: Option<&str>) -> Result<Lunch, (StatusCode, Value)> {
        let mut request = axum::http::Request::post("/x").header("content-type", "application/json");
        if let Some(length) = content_length {
            request = request.header("content-length", length);
        }
        match Json::<Lunch>::from_request(request.body(body.into()).unwrap(), &()).await {
            Ok(Json(lunch)) => Ok(lunch),
            Err(problem) => {
                let response = problem.into_response();
                let status = response.status();
                let body = axum::body::to_bytes(response.into_body(), 4096).await.unwrap();
                Err((status, serde_json::from_slice(&body).unwrap()))
            }
        }
    }

    #[tokio::test]
    async fn a_valid_body_is_parsed_and_the_last_duplicate_wins() {
        let lunch = parse(r#"{"merchant":"x","merchant":"Cafe","count":2}"#, None)
            .await
            .unwrap();
        assert_eq!(lunch.merchant.as_deref(), Some("Cafe"));
    }

    #[tokio::test]
    async fn every_broken_rule_is_reported_at_once() {
        let (status, body) = parse(r#"{"merchant":"","note":"four"}"#, None).await.unwrap_err();

        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["code"], "VALIDATION_FAILED");
        assert_eq!(
            body["errors"],
            json!([
                {"field": "merchant", "message": "size must be between 1 and 5"},
                {"field": "note", "message": "size must be between 0 and 3"},
                {"field": "count", "message": "must not be null"},
            ])
        );
    }

    #[tokio::test]
    async fn sizes_count_utf16_code_units_like_java() {
        // Two emoji are four UTF-16 units: too long for 3, though only two characters.
        let (_, body) = parse(r#"{"merchant":"a","note":"😀😀","count":1}"#, None)
            .await
            .unwrap_err();
        assert_eq!(body["errors"][0]["field"], "note");
    }

    #[tokio::test]
    async fn unreadable_bodies_are_malformed() {
        for body in [
            "",
            "{not json",
            r#"{"merchant":"Cafe","count":1,"status":"APPROVED"}"#,
            r#"{"merchant":5,"count":1}"#,
            r#"{"merchant":"Cafe","count":-1}"#,
            "[]",
        ] {
            let (status, problem) = parse(body, None).await.unwrap_err();
            assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
            assert_eq!(problem["code"], "MALFORMED_REQUEST", "{body}");
            assert_eq!(
                problem["detail"],
                "The request body is missing, malformed or contains unknown fields"
            );
        }
    }

    #[tokio::test]
    async fn a_body_over_the_limit_is_413_whether_declared_or_streamed() {
        let large = format!(r#"{{"merchant":"{}"}}"#, "x".repeat(MAX_JSON_BODY_SIZE));
        let declared = (MAX_JSON_BODY_SIZE + 1).to_string();
        for content_length in [Some(declared.as_str()), None] {
            let (status, problem) = parse(large.clone(), content_length).await.unwrap_err();
            assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
            assert_eq!(problem["code"], "PAYLOAD_TOO_LARGE");
            assert_eq!(problem["detail"], "The request body is larger than 1048576 bytes");
        }
    }

    #[test]
    fn a_path_parameter_must_be_a_canonical_uuid() {
        let id = "0190f1c2-7b6e-7c3a-9d4e-1234567890ab";
        let parameters = |value: &str| PathParameters::for_tests(&[("expenseId", value)]);
        assert_eq!(uuid_parameter(&parameters(id), "expenseId").unwrap().to_string(), id);
        assert!(uuid_parameter(&parameters(&id.to_uppercase()), "expenseId").is_ok());
        for bad in [
            "abc",
            "0190f1c27b6e7c3a9d4e1234567890ab",
            "{0190f1c2-7b6e-7c3a-9d4e-1234567890ab}",
        ] {
            let problem = uuid_parameter(&parameters(bad), "expenseId").unwrap_err();
            assert_eq!(problem.status(), StatusCode::BAD_REQUEST, "{bad}");
            assert_eq!(problem.code().code(), "REQUEST_REJECTED");
            assert_eq!(
                problem.detail(),
                Some(format!("Failed to convert 'expenseId' with value: '{bad}'").as_str())
            );
        }
    }

    #[test]
    fn a_header_is_read_as_latin_1() {
        let mut headers = HeaderMap::new();
        headers.insert("if-match", axum::http::HeaderValue::from_bytes(b"\"1\"\xe9").unwrap());
        assert_eq!(header_text(&headers, "if-match").as_deref(), Some("\"1\"é"));
        assert_eq!(header_text(&headers, "idempotency-key"), None);
    }
}
