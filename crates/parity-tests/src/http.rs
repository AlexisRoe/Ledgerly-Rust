//! A small HTTP client for tests: no redirects, no cookie jar, no retries, so a test sees exactly what the
//! server answered. Responses are checked against the spec unless the test opts out.

use std::time::Duration;

use reqwest::Method;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use serde_json::Value;

use crate::spec::Spec;

/// Sends requests to one base URL (the API port or the management port).
#[derive(Debug, Clone)]
pub struct Client {
    http: reqwest::Client,
    base_url: String,
    bearer: Option<String>,
    default_headers: Vec<(String, String)>,
}

impl Client {
    #[must_use]
    pub fn new(base_url: &str) -> Self {
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(30))
            .build()
            .expect("the HTTP client can be built");
        Self {
            http,
            base_url: base_url.trim_end_matches('/').to_owned(),
            bearer: None,
            default_headers: Vec::new(),
        }
    }

    /// The same client, sending this header with every request.
    #[must_use]
    pub fn with_default_header(mut self, name: &str, value: &str) -> Self {
        self.default_headers.push((name.to_owned(), value.to_owned()));
        self
    }

    /// The same client, sending `Authorization: Bearer <token>` with every request.
    #[must_use]
    pub fn with_bearer(mut self, token: &str) -> Self {
        self.bearer = Some(token.to_owned());
        self
    }

    /// Starts a request. `path` begins with `/` and includes the server prefix: `/api/v1/me`.
    pub fn request(&self, method: Method, path: &str) -> Request {
        let mut builder = self.http.request(method.clone(), format!("{}{path}", self.base_url));
        if let Some(token) = &self.bearer {
            builder = builder.bearer_auth(token);
        }
        for (name, value) in &self.default_headers {
            builder = builder.header(name, value);
        }
        Request {
            builder,
            method,
            path: path.to_owned(),
        }
    }

    pub fn get(&self, path: &str) -> Request {
        self.request(Method::GET, path)
    }

    pub fn post(&self, path: &str) -> Request {
        self.request(Method::POST, path)
    }

    pub fn patch(&self, path: &str) -> Request {
        self.request(Method::PATCH, path)
    }

    pub fn put(&self, path: &str) -> Request {
        self.request(Method::PUT, path)
    }

    pub fn delete(&self, path: &str) -> Request {
        self.request(Method::DELETE, path)
    }

    pub fn options(&self, path: &str) -> Request {
        self.request(Method::OPTIONS, path)
    }
}

/// A request being built.
#[derive(Debug)]
#[must_use = "a request does nothing until it is sent"]
pub struct Request {
    builder: reqwest::RequestBuilder,
    method: Method,
    path: String,
}

impl Request {
    pub fn header(mut self, name: &str, value: &str) -> Self {
        self.builder = self.builder.header(name, value);
        self
    }

    /// Sends `value` as the JSON body, with `Content-Type: application/json`.
    pub fn json(mut self, value: &Value) -> Self {
        self.builder = self.builder.json(value);
        self
    }

    /// Sends raw bytes with the given content type: malformed JSON, oversized bodies, CSV.
    pub fn body(mut self, content_type: &str, bytes: impl Into<Vec<u8>>) -> Self {
        self.builder = self.builder.header("Content-Type", content_type).body(bytes.into());
        self
    }

    /// Sends a `multipart/form-data` body with one file part, as the receipt and import uploads do.
    pub fn file(mut self, field: &str, file_name: &str, content_type: &str, bytes: impl Into<Vec<u8>>) -> Self {
        let part = reqwest::multipart::Part::bytes(bytes.into())
            .file_name(file_name.to_owned())
            .mime_str(content_type)
            .expect("the content type of the file part is valid");
        self.builder = self
            .builder
            .multipart(reqwest::multipart::Form::new().part(field.to_owned(), part));
        self
    }

    /// Sends the request and checks the response against the spec.
    ///
    /// # Panics
    ///
    /// Panics if the server cannot be reached, or if the response breaks the contract. Use
    /// [`Request::send_unchecked`] for a path the spec does not describe.
    pub async fn send(self) -> Response {
        let (method, path) = (self.method.clone(), self.path.clone());
        let response = self.send_unchecked().await;
        if let Err(violation) = Spec::vendored().check(
            method.as_str(),
            &path,
            response.status,
            response.header("content-type"),
            &response.body,
        ) {
            panic!("the response breaks api/openapi.yaml: {violation}");
        }
        response
    }

    /// Sends the request without looking at the spec: for the management port, for paths that do not exist,
    /// and for Swagger UI.
    ///
    /// # Panics
    ///
    /// Panics if the server cannot be reached.
    pub async fn send_unchecked(self) -> Response {
        let context = format!("{} {}", self.method, self.path);
        let response = self
            .builder
            .send()
            .await
            .unwrap_or_else(|error| panic!("{context}: no response ({error}). Is the app running?"));
        let status = response.status().as_u16();
        let headers = response.headers().clone();
        let body = response
            .bytes()
            .await
            .unwrap_or_else(|error| panic!("{context}: the body could not be read ({error})"))
            .to_vec();
        Response { status, headers, body }
    }
}

/// What the server answered.
#[derive(Debug, Clone)]
pub struct Response {
    pub status: u16,
    pub headers: HeaderMap,
    pub body: Vec<u8>,
}

impl Response {
    /// The first value of a header, if it is there and is text.
    #[must_use]
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name).and_then(|value| value.to_str().ok())
    }

    /// Every value of a header, such as each `Set-Cookie`.
    #[must_use]
    pub fn header_values(&self, name: &str) -> Vec<&str> {
        let Ok(name) = HeaderName::try_from(name) else {
            return Vec::new();
        };
        self.headers
            .get_all(name)
            .iter()
            .filter_map(|value: &HeaderValue| value.to_str().ok())
            .collect()
    }

    /// The media type without parameters, lower-case: `application/problem+json`.
    #[must_use]
    pub fn media_type(&self) -> Option<String> {
        self.header("content-type")
            .map(|value| value.split(';').next().unwrap_or(value).trim().to_ascii_lowercase())
    }

    #[must_use]
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }

    /// The body as JSON.
    ///
    /// # Panics
    ///
    /// Panics, quoting the body, if it is not JSON.
    #[must_use]
    pub fn json(&self) -> Value {
        serde_json::from_slice(&self.body)
            .unwrap_or_else(|error| panic!("the body is not JSON ({error}): {}", self.text()))
    }

    /// The names of the top-level properties of a JSON object body, in the order the server wrote them.
    /// (`json()` cannot tell: a parsed object is sorted by name.)
    ///
    /// # Panics
    ///
    /// Panics, quoting the body, if it is not a JSON object.
    #[must_use]
    pub fn property_names(&self) -> Vec<String> {
        struct Names;
        impl<'de> serde::de::Visitor<'de> for Names {
            type Value = Vec<String>;

            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("a JSON object")
            }

            fn visit_map<A: serde::de::MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                let mut names = Vec::new();
                while let Some(name) = map.next_key::<String>()? {
                    map.next_value::<serde::de::IgnoredAny>()?;
                    names.push(name);
                }
                Ok(names)
            }
        }
        let mut deserializer = serde_json::Deserializer::from_slice(&self.body);
        serde::Deserializer::deserialize_map(&mut deserializer, Names)
            .unwrap_or_else(|error| panic!("the body is not a JSON object ({error}): {}", self.text()))
    }

    /// The `code` of a problem body, or `None` if the body is not a problem document with one.
    #[must_use]
    pub fn problem_code(&self) -> Option<String> {
        let body: Value = serde_json::from_slice(&self.body).ok()?;
        body.get("code")?.as_str().map(str::to_owned)
    }

    /// The value of a cookie the response sets, without its attributes.
    #[must_use]
    pub fn cookie(&self, name: &str) -> Option<String> {
        self.header_values("set-cookie").into_iter().find_map(|cookie| {
            let (pair, _attributes) = cookie.split_once(';').unwrap_or((cookie, ""));
            let (key, value) = pair.split_once('=')?;
            (key.trim() == name).then(|| value.trim().to_owned())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn response(headers: &[(&'static str, &'static str)], body: &str) -> Response {
        let mut map = HeaderMap::new();
        for (name, value) in headers {
            map.append(*name, HeaderValue::from_static(value));
        }
        Response {
            status: 200,
            headers: map,
            body: body.as_bytes().to_vec(),
        }
    }

    #[test]
    fn the_media_type_drops_parameters_and_case() {
        let response = response(&[("content-type", "Application/Problem+JSON;charset=UTF-8")], "");

        assert_eq!(response.media_type().as_deref(), Some("application/problem+json"));
    }

    #[test]
    fn a_cookie_is_found_among_several_set_cookie_headers() {
        let response = response(
            &[
                ("set-cookie", "other=1; Path=/"),
                (
                    "set-cookie",
                    "refresh_token=abc123; Path=/api/v1/auth; HttpOnly; Secure; SameSite=Strict",
                ),
            ],
            "",
        );

        assert_eq!(response.cookie("refresh_token").as_deref(), Some("abc123"));
        assert_eq!(response.cookie("missing"), None);
        assert_eq!(response.header_values("set-cookie").len(), 2);
    }

    #[test]
    fn property_names_keep_the_order_of_the_body() {
        let response = response(
            &[],
            r#"{"id":"1","email":"a@b.c","role":"MEMBER","status":{"z":1,"a":2}}"#,
        );

        assert_eq!(response.property_names(), ["id", "email", "role", "status"]);
    }

    #[test]
    fn the_problem_code_is_read_from_a_problem_body_only() {
        assert_eq!(
            response(&[], r#"{"code":"NOT_FOUND","status":404}"#)
                .problem_code()
                .as_deref(),
            Some("NOT_FOUND")
        );
        assert_eq!(response(&[], "<html>").problem_code(), None);
        assert_eq!(response(&[], "{}").problem_code(), None);
    }
}
