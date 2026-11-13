//! Checks responses against the vendored `api/openapi.yaml`.

use std::sync::OnceLock;

use serde_json::{Map, Value, json};

/// The contract both implementations serve, read once per test process.
#[derive(Debug)]
pub struct Spec {
    document: Value,
    operations: Vec<Operation>,
}

#[derive(Debug)]
struct Operation {
    method: String,
    /// The full path as a client sends it, server prefix included: `/api/v1/teams/{teamId}/expenses`.
    template: String,
    responses: Value,
}

/// Why a response does not match the contract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Violation {
    /// The spec has no operation for this method and path. The test should have used an unchecked request.
    NoSuchOperation(String),
    /// The operation documents neither this status nor `default`, and the body is not a problem document.
    UndocumentedStatus(String),
    /// The media type is not one the response documents.
    WrongMediaType(String),
    /// The body does not match the schema, or is not JSON at all.
    WrongBody(String),
    /// The response documents no content, but there is a body.
    UnexpectedBody(String),
}

impl std::fmt::Display for Violation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (Self::NoSuchOperation(text)
        | Self::UndocumentedStatus(text)
        | Self::WrongMediaType(text)
        | Self::WrongBody(text)
        | Self::UnexpectedBody(text)) = self;
        f.write_str(text)
    }
}

const SOURCE: &str = include_str!("../../../api/openapi.yaml");
const METHODS: [&str; 7] = ["get", "put", "post", "delete", "options", "head", "patch"];

impl Spec {
    /// The vendored contract.
    #[must_use]
    pub fn vendored() -> &'static Self {
        static SPEC: OnceLock<Spec> = OnceLock::new();
        SPEC.get_or_init(|| Self::parse(SOURCE))
    }

    fn parse(yaml: &str) -> Self {
        let document: Value = serde_saphyr::from_str(yaml).expect("api/openapi.yaml is valid YAML");
        let default_prefix = server_prefix(&document);
        let mut operations = Vec::new();
        for (path, item) in document["paths"].as_object().expect("the spec has paths") {
            // A path may name its own server: the JWKS document lives at the root, not under /api/v1.
            let prefix = if item.get("servers").is_some() {
                server_prefix(item)
            } else {
                default_prefix.clone()
            };
            for method in METHODS {
                if let Some(operation) = item.get(method) {
                    operations.push(Operation {
                        method: method.to_ascii_uppercase(),
                        template: format!("{prefix}{path}"),
                        responses: operation["responses"].clone(),
                    });
                }
            }
        }
        Self { document, operations }
    }

    /// Every operation as (`METHOD`, path template with the server prefix), in the order of the file.
    pub fn operations(&self) -> impl Iterator<Item = (&str, &str)> {
        self.operations
            .iter()
            .map(|operation| (operation.method.as_str(), operation.template.as_str()))
    }

    /// True if the spec describes this method and path.
    #[must_use]
    pub fn describes(&self, method: &str, path: &str) -> bool {
        self.find(method, path).is_some()
    }

    fn find(&self, method: &str, path: &str) -> Option<&Operation> {
        let path = path.split(['?', '#']).next().unwrap_or(path);
        // A literal path wins over a template that would also match it.
        self.operations
            .iter()
            .filter(|operation| operation.method.eq_ignore_ascii_case(method))
            .filter(|operation| matches_template(&operation.template, path))
            .min_by_key(|operation| operation.template.matches('{').count())
    }

    /// Checks one response: the status is documented, the media type is one of the documented ones, and a JSON
    /// body matches its schema (with `format` checked).
    ///
    /// A status the operation does not document passes if the response is a valid `application/problem+json`
    /// document: errors that the framework answers before a handler runs (405, 413, 415, 429) are not
    /// repeated under every operation.
    ///
    /// # Errors
    ///
    /// Returns the first [`Violation`] found.
    pub fn check(
        &self,
        method: &str,
        path: &str,
        status: u16,
        content_type: Option<&str>,
        body: &[u8],
    ) -> Result<(), Violation> {
        let context = format!("{method} {path} -> {status}");
        let operation = self
            .find(method, path)
            .ok_or_else(|| Violation::NoSuchOperation(format!("{context}: the spec has no such operation")))?;
        let media_type = content_type
            .map(|value| value.split(';').next().unwrap_or(value).trim().to_ascii_lowercase())
            .filter(|value| !value.is_empty());

        let documented = operation
            .responses
            .get(status.to_string())
            .or_else(|| operation.responses.get("default"))
            .map(|response| self.resolve(response));
        let Some(response) = documented else {
            if media_type.as_deref() == Some("application/problem+json") {
                return self.check_json(&context, &json!({"$ref": "#/components/schemas/Problem"}), body);
            }
            return Err(Violation::UndocumentedStatus(format!(
                "{context}: the operation documents only {:?}, and the response is not a problem document",
                operation
                    .responses
                    .as_object()
                    .map(|responses| responses.keys().collect::<Vec<_>>())
                    .unwrap_or_default()
            )));
        };

        let Some(content) = response.get("content").and_then(Value::as_object) else {
            if body.is_empty() {
                return Ok(());
            }
            return Err(Violation::UnexpectedBody(format!(
                "{context}: the spec documents no content, but the response has {} bytes",
                body.len()
            )));
        };
        let Some(media_type) = media_type else {
            return Err(Violation::WrongMediaType(format!(
                "{context}: no Content-Type, expected one of {:?}",
                content.keys().collect::<Vec<_>>()
            )));
        };
        let Some(media) = content.get(&media_type).or_else(|| content.get("*/*")) else {
            return Err(Violation::WrongMediaType(format!(
                "{context}: Content-Type {media_type}, expected one of {:?}",
                content.keys().collect::<Vec<_>>()
            )));
        };
        match media.get("schema") {
            Some(schema) if media_type.ends_with("json") => self.check_json(&context, schema, body),
            _ => Ok(()),
        }
    }

    /// Follows a `$ref` to a shared response.
    fn resolve<'a>(&'a self, value: &'a Value) -> &'a Value {
        match value.get("$ref").and_then(Value::as_str) {
            Some(reference) => self
                .document
                .pointer(reference.trim_start_matches('#'))
                .unwrap_or_else(|| panic!("the spec has a dangling reference: {reference}")),
            None => value,
        }
    }

    fn check_json(&self, context: &str, schema: &Value, body: &[u8]) -> Result<(), Violation> {
        let instance: Value = serde_json::from_slice(body).map_err(|error| {
            Violation::WrongBody(format!(
                "{context}: the body is not JSON ({error}): {}",
                String::from_utf8_lossy(body)
            ))
        })?;
        // OpenAPI 3.1 schemas are JSON Schema 2020-12. Their references point into `#/components`, so the
        // components travel along as a keyword the validator ignores but can resolve into.
        let mut root: Map<String, Value> = schema.as_object().cloned().unwrap_or_default();
        root.insert("components".to_owned(), self.document["components"].clone());
        let validator = jsonschema::options()
            .with_draft(jsonschema::Draft::Draft202012)
            .should_validate_formats(true)
            .build(&Value::Object(root))
            .unwrap_or_else(|error| panic!("{context}: the spec's schema cannot be compiled: {error}"));
        let errors: Vec<String> = validator
            .iter_errors(&instance)
            .map(|error| format!("  at `{}`: {error}", error.instance_path()))
            .collect();
        if errors.is_empty() {
            Ok(())
        } else {
            Err(Violation::WrongBody(format!(
                "{context}: the body does not match the spec:\n{}\n  body: {instance}",
                errors.join("\n")
            )))
        }
    }
}

/// The path of the first server URL: `/api/v1`, or nothing for `/`.
fn server_prefix(holder: &Value) -> String {
    holder["servers"][0]["url"]
        .as_str()
        .unwrap_or("")
        .trim_end_matches('/')
        .to_owned()
}

/// True if `path` is an instance of `template`. A `{parameter}` stands for one or more characters other than
/// `/` and `:`, so `/expenses/{expenseId}` does not match `/expenses/123:submit`.
fn matches_template(template: &str, path: &str) -> bool {
    let (mut template_segments, mut path_segments) = (template.split('/'), path.split('/'));
    loop {
        match (template_segments.next(), path_segments.next()) {
            (None, None) => return true,
            (Some(pattern), Some(actual)) if matches_segment(pattern, actual) => {}
            _ => return false,
        }
    }
}

/// One path segment: a literal, or `{parameter}` with an optional literal suffix such as `:submit`.
fn matches_segment(pattern: &str, actual: &str) -> bool {
    let Some(rest) = pattern.strip_prefix('{') else {
        return pattern == actual;
    };
    let Some((_name, suffix)) = rest.split_once('}') else {
        return false;
    };
    actual
        .strip_suffix(suffix)
        .is_some_and(|value| !value.is_empty() && !value.contains(':'))
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEAM: &str = "0196f3a2-0000-7000-8000-000000000001";

    fn spec() -> &'static Spec {
        Spec::vendored()
    }

    fn problem(status: u16, code: &str) -> Vec<u8> {
        json!({"type": "about:blank", "title": "x", "status": status, "code": code, "traceId": "abc"})
            .to_string()
            .into_bytes()
    }

    #[test]
    fn the_vendored_spec_is_read_with_its_server_prefixes() {
        let operations: Vec<_> = spec().operations().collect();

        assert!(operations.len() > 30, "{}", operations.len());
        assert!(operations.contains(&("POST", "/api/v1/auth/login")));
        assert!(operations.contains(&("GET", "/api/v1/teams/{teamId}/expenses")));
        // The JWKS path names its own server: it is at the root, not under /api/v1.
        assert!(operations.contains(&("GET", "/.well-known/jwks.json")));
    }

    #[test]
    fn paths_are_matched_against_their_templates() {
        let expense = format!("/api/v1/teams/{TEAM}/expenses/{TEAM}");
        assert!(spec().describes("GET", &expense));
        assert!(spec().describes("get", &format!("{expense}?x=1")));
        assert!(spec().describes("POST", &format!("{expense}:submit")));
        assert!(spec().describes("POST", &format!("/api/v1/teams/{TEAM}/expenses:batch")));
        assert!(spec().describes("GET", &format!("/api/v1/teams/{TEAM}/expenses.csv")));
        assert!(spec().describes("POST", &format!("/api/v1/teams/{TEAM}:leave")));

        assert!(
            !spec().describes("GET", &format!("{expense}:submit")),
            "submit is POST only"
        );
        assert!(!spec().describes("POST", &format!("{expense}:explode")));
        assert!(!spec().describes("GET", "/api/v1/nothing-here"));
        assert!(
            !spec().describes("GET", "/auth/login"),
            "the server prefix is part of the path"
        );
        assert!(!spec().describes("GET", "/api/v1/.well-known/jwks.json"));
        assert!(!spec().describes("GET", &format!("/api/v1/teams/{TEAM}/expenses/")));
    }

    #[test]
    fn segments_with_an_action_suffix() {
        assert!(matches_segment("{id}:submit", "42:submit"));
        assert!(!matches_segment("{id}:submit", ":submit"));
        assert!(!matches_segment("{id}", "42:submit"));
        assert!(matches_segment("expenses:batch", "expenses:batch"));
        assert!(!matches_segment("expenses", "expenses:batch"));
    }

    #[test]
    fn a_documented_response_with_a_matching_body_passes() {
        let body = json!({"message": "If this address can be registered, a verification message has been sent."});

        let result = spec().check(
            "POST",
            "/api/v1/auth/register",
            202,
            Some("application/json"),
            body.to_string().as_bytes(),
        );

        assert_eq!(result, Ok(()));
    }

    #[test]
    fn a_body_that_breaks_the_schema_is_reported_with_the_place() {
        let result = spec().check(
            "POST",
            "/api/v1/auth/register",
            202,
            Some("application/json"),
            br#"{"message": 42}"#,
        );

        let Err(Violation::WrongBody(message)) = result else {
            panic!("expected a body violation, got {result:?}")
        };
        assert!(message.contains("/message"), "{message}");
    }

    #[test]
    fn a_shared_problem_response_is_resolved_and_checked() {
        let path = "/api/v1/auth/login";

        assert_eq!(
            spec().check(
                "POST",
                path,
                401,
                Some("application/problem+json"),
                &problem(401, "UNAUTHENTICATED")
            ),
            Ok(())
        );
        // `status` must be a number.
        let broken = br#"{"type":"about:blank","title":"x","status":"401","code":"UNAUTHENTICATED"}"#;
        assert!(matches!(
            spec().check("POST", path, 401, Some("application/problem+json"), broken),
            Err(Violation::WrongBody(_))
        ));
        assert!(matches!(
            spec().check(
                "POST",
                path,
                401,
                Some("application/json"),
                &problem(401, "UNAUTHENTICATED")
            ),
            Err(Violation::WrongMediaType(_))
        ));
    }

    #[test]
    fn the_charset_parameter_of_a_content_type_is_ignored() {
        assert_eq!(
            spec().check(
                "POST",
                "/api/v1/auth/login",
                401,
                Some("application/problem+json;charset=UTF-8"),
                &problem(401, "UNAUTHENTICATED")
            ),
            Ok(())
        );
    }

    #[test]
    fn an_undocumented_status_passes_only_as_a_problem_document() {
        let path = "/api/v1/auth/login";

        assert_eq!(
            spec().check(
                "POST",
                path,
                418,
                Some("application/problem+json"),
                &problem(418, "TEAPOT")
            ),
            Ok(())
        );
        assert!(matches!(
            spec().check("POST", path, 418, Some("text/html"), b"<h1>Whitelabel Error Page</h1>"),
            Err(Violation::UndocumentedStatus(_))
        ));
        assert!(matches!(
            spec().check("POST", path, 418, Some("application/problem+json"), b"{}"),
            Err(Violation::WrongBody(_))
        ));
    }

    #[test]
    fn a_response_without_documented_content_must_be_empty() {
        let path = "/api/v1/auth/logout";

        assert_eq!(spec().check("POST", path, 204, None, b""), Ok(()));
        assert!(matches!(
            spec().check("POST", path, 204, Some("application/json"), b"{}"),
            Err(Violation::UnexpectedBody(_))
        ));
    }

    #[test]
    fn a_path_outside_the_spec_is_its_own_violation() {
        assert!(matches!(
            spec().check("GET", "/actuator/health", 200, Some("application/json"), b"{}"),
            Err(Violation::NoSuchOperation(_))
        ));
    }

    #[test]
    fn formats_are_checked() {
        let me =
            |id: &str| json!({"id": id, "email": "a@example.com", "verified": true, "teams": [], "invitations": []});
        let check = |body: Value| {
            spec().check(
                "GET",
                "/api/v1/me",
                200,
                Some("application/json"),
                body.to_string().as_bytes(),
            )
        };

        assert_eq!(check(me(TEAM)), Ok(()));
        assert!(matches!(check(me("not-a-uuid")), Err(Violation::WrongBody(_))));
    }
}
