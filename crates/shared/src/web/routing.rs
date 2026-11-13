//! The route table every module adds its endpoints to, and the matching that picks one for a request.
//!
//! Java counterpart: Spring MVC's `RequestMappingHandlerMapping` with path patterns, as the generated
//! `*Api` interfaces declare them. axum's own router cannot be used: it refuses a parameter followed by text
//! in the same segment, which every `:action` path has (ADR 0014).
//!
//! The rules copied from Spring:
//! - `{name}` captures one whole segment, `{name}:suffix` a segment that ends in the suffix, minus the suffix.
//! - Of all patterns that match, the most specific wins: fewer captures first, then more literal characters.
//!   So `abc:approve` goes to `{expenseId}:approve`, not to `{expenseId}`.
//! - No pattern matches: 404. A pattern matches, but not with this method: 405 with `Allow`. The endpoint
//!   needs a JSON body and the request is not JSON: 415.
//! - `HEAD` is answered by the `GET` endpoint.

use std::convert::Infallible;
use std::fmt;
use std::sync::Arc;
use std::task::{Context, Poll};

use axum::extract::{FromRequestParts, Request};
use axum::handler::Handler;
use axum::http::request::Parts;
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, header};
use axum::response::{IntoResponse, Response};
use tower::util::BoxCloneSyncService;
use tower::{Service, ServiceExt};

use super::problem::Problem;

/// Every endpoint of the API, built up by the modules and turned into one service by the app.
#[derive(Clone, Default)]
pub struct Routes {
    endpoints: Vec<Endpoint>,
}

#[derive(Clone)]
struct Endpoint {
    method: Method,
    template: &'static str,
    pattern: Pattern,
    body: Body,
    service: BoxCloneSyncService<Request, Response, Infallible>,
}

/// What an endpoint reads from the request body. Java: the `consumes` of a `@RequestMapping`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Body {
    /// Any body, or none; it is not read.
    Ignored,
    /// A JSON body: a request whose `Content-Type` is not `application/json` gets 415.
    Json,
}

impl Routes {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds an endpoint. `template` is the full path a client sends, `/api/v1` included, with `{name}` for each
    /// parameter, exactly as the spec writes it.
    #[must_use]
    pub fn route<H, T, S>(mut self, method: Method, template: &'static str, body: Body, handler: H, state: S) -> Self
    where
        H: Handler<T, S>,
        T: 'static,
        S: Clone + Send + Sync + 'static,
    {
        self.endpoints.push(Endpoint {
            method,
            template,
            pattern: Pattern::parse(template),
            body,
            service: BoxCloneSyncService::new(handler.with_state(state)),
        });
        self
    }

    /// The endpoints of both tables.
    #[must_use]
    pub fn merge(mut self, other: Self) -> Self {
        self.endpoints.extend(other.endpoints);
        self
    }

    /// Method and path template of every endpoint, in the order they were added. The inventory test compares
    /// them with the spec (OWASP API9).
    pub fn operations(&self) -> impl Iterator<Item = (&Method, &'static str)> {
        self.endpoints
            .iter()
            .map(|endpoint| (&endpoint.method, endpoint.template))
    }

    /// The table as one service. A request no endpoint takes is answered with the [`RouteRejection`]'s
    /// response.
    #[must_use]
    pub fn into_service(self) -> RouteService {
        RouteService {
            endpoints: self.endpoints.into(),
        }
    }
}

impl fmt::Debug for Routes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.operations()).finish()
    }
}

/// The route table as a `tower::Service`.
#[derive(Clone)]
pub struct RouteService {
    endpoints: Arc<[Endpoint]>,
}

impl fmt::Debug for RouteService {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RouteService")
            .field("endpoints", &self.endpoints.len())
            .finish()
    }
}

impl Service<Request> for RouteService {
    type Response = Response;
    type Error = Infallible;
    type Future = std::pin::Pin<Box<dyn Future<Output = Result<Response, Infallible>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Infallible>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, mut request: Request) -> Self::Future {
        let chosen = select(
            &self.endpoints,
            request.method(),
            request.uri().path(),
            request.headers(),
        );
        match chosen {
            Ok((endpoint, parameters)) => {
                request.extensions_mut().insert(parameters);
                let service = endpoint.service.clone();
                Box::pin(service.oneshot(request))
            }
            Err(rejection) => {
                let response = rejection.into_response();
                Box::pin(std::future::ready(Ok(response)))
            }
        }
    }
}

/// Why no endpoint takes the request. Java: the exceptions Spring's handler mapping throws.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RouteRejection {
    /// No pattern matches the path. Java: `NoResourceFoundException`, from the static-resource handler that
    /// gets every request no controller takes.
    NotFound { path: String },
    /// The path matches, the method does not. Java: `HttpRequestMethodNotSupportedException`.
    MethodNotAllowed { method: Method, allowed: Vec<Method> },
    /// The endpoint reads JSON, the request is something else. Java: `HttpMediaTypeNotSupportedException`.
    UnsupportedMediaType { content_type: String },
}

/// The problem bodies Spring writes for these, with `code: REQUEST_REJECTED` (`ProblemFactory.enrich`).
impl IntoResponse for RouteRejection {
    fn into_response(self) -> Response {
        let problem = match self {
            // Spring's static-resource handler names the path without its leading slash.
            Self::NotFound { path } => Problem::rejected(
                StatusCode::NOT_FOUND,
                format!("No static resource {}.", path.trim_start_matches('/')),
            ),
            Self::MethodNotAllowed { method, allowed } => {
                let allow = allowed.iter().map(Method::as_str).collect::<Vec<_>>().join(",");
                let problem = Problem::rejected(
                    StatusCode::METHOD_NOT_ALLOWED,
                    format!("Method '{method}' is not supported."),
                );
                match HeaderValue::from_str(&allow) {
                    Ok(allow) => problem.with_header(header::ALLOW, allow),
                    Err(_) => problem,
                }
            }
            Self::UnsupportedMediaType { content_type } => Problem::rejected(
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                format!("Content-Type '{content_type}' is not supported."),
            )
            .with_header(header::ACCEPT, HeaderValue::from_static("application/json")),
        };
        problem.into_response()
    }
}

/// The endpoint for this request with its path parameters, or why there is none.
fn select<'a>(
    endpoints: &'a [Endpoint],
    method: &Method,
    path: &str,
    headers: &HeaderMap,
) -> Result<(&'a Endpoint, PathParameters), RouteRejection> {
    let matching: Vec<(&Endpoint, PathParameters)> = endpoints
        .iter()
        .filter_map(|endpoint| endpoint.pattern.matches(path).map(|parameters| (endpoint, parameters)))
        .collect();
    if matching.is_empty() {
        return Err(RouteRejection::NotFound { path: path.to_owned() });
    }
    let wanted = if method == Method::HEAD && !matching.iter().any(|(e, _)| e.method == Method::HEAD) {
        Method::GET
    } else {
        method.clone()
    };
    let best = matching
        .iter()
        .filter(|(endpoint, _)| endpoint.method == wanted)
        .min_by_key(|(endpoint, _)| endpoint.pattern.specificity());
    let Some((endpoint, parameters)) = best else {
        let mut allowed: Vec<Method> = Vec::new();
        for (endpoint, _) in &matching {
            if !allowed.contains(&endpoint.method) {
                allowed.push(endpoint.method.clone());
            }
        }
        return Err(RouteRejection::MethodNotAllowed {
            method: method.clone(),
            allowed,
        });
    };
    if endpoint.body == Body::Json {
        let content_type = content_type(headers);
        if !is_json(&content_type) {
            return Err(RouteRejection::UnsupportedMediaType { content_type });
        }
    }
    Ok((endpoint, parameters.clone()))
}

/// The request's media type as Spring prints it: type and subtype in lower case, parameters joined by `;`.
/// A request without one counts as `application/octet-stream`, as in Spring.
fn content_type(headers: &HeaderMap) -> String {
    let Some(value) = headers.get(header::CONTENT_TYPE) else {
        return "application/octet-stream".to_owned();
    };
    let text = String::from_utf8_lossy(value.as_bytes());
    let mut parts = text.split(';').map(str::trim).filter(|part| !part.is_empty());
    let essence = parts.next().unwrap_or_default().to_ascii_lowercase();
    std::iter::once(essence)
        .chain(parts.map(str::to_owned))
        .collect::<Vec<_>>()
        .join(";")
}

fn is_json(content_type: &str) -> bool {
    content_type.split(';').next() == Some("application/json")
}

/// The values of the `{name}` parameters of the matched path, percent-decoded.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PathParameters(Vec<(&'static str, String)>);

impl PathParameters {
    /// The value of the parameter called `name`, if the endpoint's template has one.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&str> {
        self.0
            .iter()
            .find(|(known, _)| *known == name)
            .map(|(_, value)| value.as_str())
    }

    #[cfg(test)]
    pub(crate) fn for_tests(pairs: &[(&'static str, &str)]) -> Self {
        Self(pairs.iter().map(|(name, value)| (*name, (*value).to_owned())).collect())
    }
}

/// Handlers take the parameters as an extractor. Outside the route table there are none.
impl<S: Send + Sync> FromRequestParts<S> for PathParameters {
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Infallible> {
        Ok(parts.extensions.get::<Self>().cloned().unwrap_or_default())
    }
}

/// A parsed template: one entry per path segment.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Pattern(Vec<Segment>);

#[derive(Debug, Clone, PartialEq, Eq)]
enum Segment {
    Literal(&'static str),
    /// `{name}` followed by a literal suffix, which may be empty.
    Capture {
        name: &'static str,
        suffix: &'static str,
    },
}

impl Pattern {
    /// Splits a template into segments. A segment that starts with `{` and has a `}` is a capture; everything
    /// else, including a malformed capture, is literal text, so parsing cannot fail.
    fn parse(template: &'static str) -> Self {
        let segments = template
            .split('/')
            .map(|segment| {
                segment.strip_prefix('{').and_then(|rest| rest.split_once('}')).map_or(
                    Segment::Literal(segment),
                    |(name, suffix)| Segment::Capture { name, suffix },
                )
            })
            .collect();
        Self(segments)
    }

    fn matches(&self, path: &str) -> Option<PathParameters> {
        let segments: Vec<&str> = path.split('/').collect();
        if segments.len() != self.0.len() {
            return None;
        }
        let mut parameters = Vec::new();
        for (expected, actual) in self.0.iter().zip(segments) {
            match expected {
                Segment::Literal(literal) => {
                    if *literal != actual {
                        return None;
                    }
                }
                Segment::Capture { name, suffix } => {
                    let value = actual.strip_suffix(suffix).filter(|value| !value.is_empty())?;
                    parameters.push((*name, percent_decode(value)));
                }
            }
        }
        Some(PathParameters(parameters))
    }

    /// Lower is more specific: fewer captures, then more literal characters.
    fn specificity(&self) -> (usize, std::cmp::Reverse<usize>) {
        let captures = self
            .0
            .iter()
            .filter(|segment| matches!(segment, Segment::Capture { .. }))
            .count();
        let literal_length = self
            .0
            .iter()
            .map(|segment| match segment {
                Segment::Literal(literal) => literal.len(),
                Segment::Capture { suffix, .. } => suffix.len(),
            })
            .sum();
        (captures, std::cmp::Reverse(literal_length))
    }
}

/// Decodes `%XX` escapes as UTF-8. An escape that is not two hex digits stays as it is, and bytes that are not
/// UTF-8 become U+FFFD; such a value fails the parameter's own parsing later.
fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut rest = bytes;
    while let Some((&first, tail)) = rest.split_first() {
        let escaped = match (first, tail) {
            (b'%', [high, low, ..]) => hex(*high).zip(hex(*low)).map(|(high, low)| (high << 4) | low),
            _ => None,
        };
        match (escaped, tail) {
            (Some(byte), [_, _, after @ ..]) => {
                decoded.push(byte);
                rest = after;
            }
            _ => {
                decoded.push(first);
                rest = tail;
            }
        }
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

const fn hex(digit: u8) -> Option<u8> {
    match digit {
        b'0'..=b'9' => Some(digit - b'0'),
        b'a'..=b'f' => Some(digit - b'a' + 10),
        b'A'..=b'F' => Some(digit - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use axum::body::Body as HttpBody;

    use super::*;

    const EXPENSE: &str = "/api/v1/teams/{teamId}/expenses/{expenseId}";
    const APPROVE: &str = "/api/v1/teams/{teamId}/expenses/{expenseId}:approve";

    /// Each handler answers with its name and the parameters it got.
    fn table() -> RouteService {
        fn named(name: &'static str) -> impl Fn(PathParameters) -> std::future::Ready<String> + Clone {
            move |parameters: PathParameters| {
                let team = parameters.get("teamId").unwrap_or("-").to_owned();
                let expense = parameters.get("expenseId").unwrap_or("-").to_owned();
                std::future::ready(format!("{name} {team} {expense}"))
            }
        }
        Routes::new()
            .route(Method::GET, EXPENSE, Body::Ignored, named("get"), ())
            .route(Method::PATCH, EXPENSE, Body::Json, named("patch"), ())
            .route(Method::DELETE, EXPENSE, Body::Ignored, named("delete"), ())
            .route(Method::POST, APPROVE, Body::Ignored, named("approve"), ())
            .route(
                Method::POST,
                "/api/v1/teams/{teamId}/expenses",
                Body::Json,
                named("create"),
                (),
            )
            .route(
                Method::POST,
                "/api/v1/teams/{teamId}/expenses:batch",
                Body::Json,
                named("batch"),
                (),
            )
            .route(
                Method::POST,
                "/api/v1/teams/{teamId}:leave",
                Body::Ignored,
                named("leave"),
                (),
            )
            .into_service()
    }

    async fn send(method: Method, path: &str, content_type: Option<&str>) -> (StatusCode, HeaderMap, String) {
        let mut request = axum::http::Request::builder().method(method).uri(path);
        if let Some(content_type) = content_type {
            request = request.header(header::CONTENT_TYPE, content_type);
        }
        let response = table().oneshot(request.body(HttpBody::empty()).unwrap()).await.unwrap();
        let status = response.status();
        let headers = response.headers().clone();
        let body = axum::body::to_bytes(response.into_body(), 1024).await.unwrap();
        (status, headers, String::from_utf8(body.to_vec()).unwrap())
    }

    #[tokio::test]
    async fn a_parameter_with_a_suffix_matches_and_the_more_specific_pattern_wins() {
        let (status, _, body) = send(Method::POST, "/api/v1/teams/t1/expenses/e1:approve", None).await;
        assert_eq!((status, body.as_str()), (StatusCode::OK, "approve t1 e1"));

        let (_, _, body) = send(Method::GET, "/api/v1/teams/t1/expenses/e1", None).await;
        assert_eq!(body, "get t1 e1");

        let (_, _, body) = send(Method::POST, "/api/v1/teams/t1:leave", None).await;
        assert_eq!(body, "leave t1 -");
    }

    #[tokio::test]
    async fn a_literal_with_a_colon_is_a_literal() {
        let (_, _, body) = send(
            Method::POST,
            "/api/v1/teams/t1/expenses:batch",
            Some("application/json"),
        )
        .await;
        assert_eq!(body, "batch t1 -");
    }

    #[tokio::test]
    async fn without_its_own_method_the_suffix_is_part_of_the_plain_parameter_as_in_spring() {
        // Spring: `GET .../{expenseId}` matches `e1:approve` as the id, which then fails to parse as a UUID.
        let (_, _, body) = send(Method::GET, "/api/v1/teams/t1/expenses/e1:approve", None).await;
        assert_eq!(body, "get t1 e1:approve");
    }

    #[tokio::test]
    async fn an_unknown_path_is_404() {
        for path in [
            "/api/v1/nope",
            "/api/v1/teams/t1/expenses/e1/",
            "/api/v1/teams//expenses/e1",
            "/api/v1/teams/t1/expenses/e1:nope/x",
        ] {
            let (status, ..) = send(Method::GET, path, None).await;
            assert_eq!(status, StatusCode::NOT_FOUND, "{path}");
        }
    }

    #[tokio::test]
    async fn a_known_path_with_another_method_is_405_listing_the_allowed_methods() {
        let (status, headers, _) = send(Method::PUT, "/api/v1/teams/t1/expenses/e1", None).await;
        assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);
        assert_eq!(headers[header::ALLOW], "GET,PATCH,DELETE");

        // An unknown action still matches the plain `{expenseId}` pattern, so Spring answers 405, not 404.
        let (status, headers, _) = send(Method::POST, "/api/v1/teams/t1/expenses/e1:nope", None).await;
        assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);
        assert_eq!(headers[header::ALLOW], "GET,PATCH,DELETE");
    }

    #[tokio::test]
    async fn head_is_answered_by_get() {
        let (status, ..) = send(Method::HEAD, "/api/v1/teams/t1/expenses/e1", None).await;
        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn a_json_endpoint_refuses_other_media_types_and_a_missing_one() {
        for (content_type, expected) in [
            (Some("application/json"), StatusCode::OK),
            (Some("Application/JSON; charset=UTF-8"), StatusCode::OK),
            (Some("text/plain"), StatusCode::UNSUPPORTED_MEDIA_TYPE),
            (Some("application/merge-patch+json"), StatusCode::UNSUPPORTED_MEDIA_TYPE),
            (None, StatusCode::UNSUPPORTED_MEDIA_TYPE),
        ] {
            let (status, ..) = send(Method::PATCH, "/api/v1/teams/t1/expenses/e1", content_type).await;
            assert_eq!(status, expected, "{content_type:?}");
        }
        let (status, ..) = send(Method::POST, "/api/v1/teams/t1/expenses/e1:approve", Some("text/plain")).await;
        assert_eq!(
            status,
            StatusCode::OK,
            "an endpoint without a body takes any media type"
        );
    }

    #[test]
    fn the_rejections_name_what_spring_names() {
        let mut headers = HeaderMap::new();
        headers.insert(header::CONTENT_TYPE, "Text/Plain; charset=utf-8".parse().unwrap());
        assert_eq!(content_type(&headers), "text/plain;charset=utf-8");
        assert_eq!(content_type(&HeaderMap::new()), "application/octet-stream");
    }

    #[tokio::test]
    async fn parameters_are_percent_decoded() {
        let (_, _, body) = send(Method::GET, "/api/v1/teams/a%2Db/expenses/%E2%82%AC%zz", None).await;
        assert_eq!(body, "get a-b €%zz");
    }

    #[test]
    fn the_operations_are_listed_in_the_order_they_were_added() {
        let routes = Routes::new()
            .route(Method::GET, EXPENSE, Body::Ignored, || async {}, ())
            .merge(Routes::new().route(Method::POST, APPROVE, Body::Ignored, || async {}, ()));
        let operations: Vec<(Method, &str)> = routes.operations().map(|(m, t)| (m.clone(), t)).collect();
        assert_eq!(operations, [(Method::GET, EXPENSE), (Method::POST, APPROVE)]);
    }
}
