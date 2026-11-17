//! Swagger UI over the vendored spec, in the `local` profile only.
//!
//! Java counterpart: `shared/web/SwaggerUiConfig.java`: `/swagger-ui/**` from Java's own `index.html`, the
//! checked-in `openapi.yaml` and the swagger-ui 5.34.1 webjar, and `/docs` redirecting to the UI. Other profiles
//! never register these routes (OWASP API8), so there `/docs` is as unknown, and from M5 as protected, as any other
//! path.
//!
//! These routes are not in the route table: they are not part of the API, and the inventory test (API9) compares
//! that table with the spec. Everything is embedded in the binary; nothing is read from disk or fetched.

use axum::Router;
use axum::extract::Path;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use ledgerly_shared::web::Problem;

/// The vendored contract, byte for byte (`Api9…theSpecTheApplicationServesIsTheFileInTheRepository`).
const SPEC: &[u8] = include_bytes!("../../../api/openapi.yaml");

/// `(file name, content type, bytes)`.
const FILES: [(&str, &str, &[u8]); 4] = [
    (
        "index.html",
        "text/html;charset=UTF-8",
        include_bytes!("../assets/swagger-ui/index.html"),
    ),
    ("openapi.yaml", "application/yaml", SPEC),
    (
        "swagger-ui-bundle.js",
        "text/javascript",
        include_bytes!("../assets/swagger-ui/swagger-ui-bundle.js"),
    ),
    (
        "swagger-ui.css",
        "text/css",
        include_bytes!("../assets/swagger-ui/swagger-ui.css"),
    ),
];

/// The routes to add to the API router when the `local` profile is active.
pub(crate) fn routes() -> Router {
    Router::new()
        .route("/docs", get(redirect))
        .route("/swagger-ui/{file}", get(file))
}

/// Spring's redirect view controller: 302 to the UI page.
async fn redirect() -> Response {
    (
        StatusCode::FOUND,
        [(header::LOCATION, HeaderValue::from_static("/swagger-ui/index.html"))],
    )
        .into_response()
}

async fn file(Path(name): Path<String>) -> Response {
    match FILES.iter().find(|(known, ..)| *known == name) {
        Some((_, content_type, bytes)) => {
            ([(header::CONTENT_TYPE, HeaderValue::from_static(content_type))], *bytes).into_response()
        }
        // What Spring's resource handler answers for a file it does not have.
        None => {
            Problem::rejected(StatusCode::NOT_FOUND, format!("No static resource swagger-ui/{name}.")).into_response()
        }
    }
}
