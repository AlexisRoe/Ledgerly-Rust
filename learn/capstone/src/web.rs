//! The HTTP layer: routes, handlers, and errors turned into `application/problem+json` responses.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use axum::extract::{Path, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

use crate::domain::{DomainError, Link, Slug, Target};
use crate::store::{LinkStore, StoreError};

/// What every handler gets. Cloned per request, so every field is cheap to clone: two `Arc`s and a channel sender.
#[derive(Debug, Clone)]
pub struct AppState {
    store: Arc<dyn LinkStore>,
    visits: mpsc::Sender<Slug>,
    next_number: Arc<AtomicU64>,
}

impl AppState {
    #[must_use]
    pub fn new(store: Arc<dyn LinkStore>, visits: mpsc::Sender<Slug>) -> Self {
        Self {
            store,
            visits,
            next_number: Arc::new(AtomicU64::new(0)),
        }
    }
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/links", post(create_link))
        .route("/links/{slug}", get(link_info))
        .route("/{slug}", get(follow))
        .with_state(state)
}

/// The request body of `POST /links`. Unknown fields are an error, not silently ignored.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateLink {
    pub target: String,
    pub slug: Option<String>,
}

/// A link as the API shows it.
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LinkView {
    pub slug: String,
    pub target: String,
    pub visits: u64,
}

impl From<&Link> for LinkView {
    fn from(link: &Link) -> Self {
        Self {
            slug: link.slug.to_string(),
            target: link.target.as_str().to_owned(),
            visits: link.visits,
        }
    }
}

async fn create_link(
    State(state): State<AppState>,
    Json(body): Json<CreateLink>,
) -> Result<(StatusCode, Json<LinkView>), ApiError> {
    let target = Target::parse(&body.target)?;
    let link = match body.slug {
        Some(slug) => {
            let link = Link::new(Slug::parse(&slug)?, target);
            state.store.insert(link.clone())?;
            link
        }
        // Generated slugs may collide with chosen ones: take the next number until one is free.
        None => loop {
            let slug = Slug::generate(state.next_number.fetch_add(1, Ordering::Relaxed));
            let link = Link::new(slug, target.clone());
            match state.store.insert(link.clone()) {
                Ok(()) => break link,
                Err(StoreError::Taken(_)) => {}
            }
        },
    };
    tracing::info!(slug = %link.slug, "link created");
    Ok((StatusCode::CREATED, Json(LinkView::from(&link))))
}

async fn link_info(State(state): State<AppState>, Path(slug): Path<String>) -> Result<Json<LinkView>, ApiError> {
    let link = find(&state, &slug)?;
    Ok(Json(LinkView::from(&link)))
}

async fn follow(State(state): State<AppState>, Path(slug): Path<String>) -> Result<Redirect, ApiError> {
    let link = find(&state, &slug)?;
    // Never wait for the counter: if its queue is full, this visit goes uncounted rather than slowing the redirect.
    if state.visits.try_send(link.slug).is_err() {
        tracing::warn!("visit not counted: the counter is behind");
    }
    Ok(Redirect::temporary(link.target.as_str()))
}

/// A slug that isn't even valid can't exist: that's a 404 too, not a 400.
fn find(state: &AppState, slug: &str) -> Result<Link, ApiError> {
    let slug = Slug::parse(slug).map_err(|_| ApiError::NotFound)?;
    state.store.get(&slug).ok_or(ApiError::NotFound)
}

/// Every way a request can fail, and the status it gets.
#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error(transparent)]
    Invalid(#[from] DomainError),
    #[error(transparent)]
    Conflict(#[from] StoreError),
    #[error("no link with this slug")]
    NotFound,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = match &self {
            Self::Invalid(_) => StatusCode::BAD_REQUEST,
            Self::Conflict(_) => StatusCode::CONFLICT,
            Self::NotFound => StatusCode::NOT_FOUND,
        };
        let problem = serde_json::json!({
            "title": status.canonical_reason().unwrap_or("Error"),
            "status": status.as_u16(),
            "detail": self.to_string(),
        });
        (
            status,
            [(header::CONTENT_TYPE, "application/problem+json")],
            Json(problem),
        )
            .into_response()
    }
}

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use axum::http::Request;
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    use super::*;
    use crate::store::MemoryStore;

    struct World {
        router: Router,
        visits: mpsc::Receiver<Slug>,
    }

    fn world() -> World {
        let (sender, visits) = mpsc::channel(16);
        let router = router(AppState::new(Arc::new(MemoryStore::default()), sender));
        World { router, visits }
    }

    async fn call(router: &Router, method: &str, uri: &str, body: Option<&str>) -> (StatusCode, Response) {
        let request = Request::builder().method(method).uri(uri);
        let request = match body {
            Some(json) => request
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(json.to_owned())),
            None => request.body(Body::empty()),
        }
        .unwrap();
        let response = router.clone().oneshot(request).await.unwrap();
        (response.status(), response)
    }

    async fn json(response: Response) -> serde_json::Value {
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        serde_json::from_slice(&bytes).unwrap()
    }

    #[tokio::test]
    async fn creates_a_link_with_a_chosen_slug() {
        let world = world();
        let (status, response) = call(
            &world.router,
            "POST",
            "/links",
            Some(r#"{"target":"https://example.org","slug":"ex"}"#),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "too short");
        assert_eq!(response.headers()[header::CONTENT_TYPE], "application/problem+json");

        let (status, response) = call(
            &world.router,
            "POST",
            "/links",
            Some(r#"{"target":"https://example.org","slug":"example"}"#),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(json(response).await["slug"], "example");

        let (status, _) = call(
            &world.router,
            "POST",
            "/links",
            Some(r#"{"target":"https://example.com","slug":"example"}"#),
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn generates_a_slug_when_none_is_given() {
        let world = world();
        let (status, response) = call(
            &world.router,
            "POST",
            "/links",
            Some(r#"{"target":"https://example.org"}"#),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(json(response).await["slug"], "0000");
    }

    #[tokio::test]
    async fn redirects_and_counts_the_visit() {
        let mut world = world();
        call(
            &world.router,
            "POST",
            "/links",
            Some(r#"{"target":"https://example.org/x","slug":"go-x"}"#),
        )
        .await;
        let (status, response) = call(&world.router, "GET", "/go-x", None).await;
        assert_eq!(status, StatusCode::TEMPORARY_REDIRECT);
        assert_eq!(response.headers()[header::LOCATION], "https://example.org/x");
        assert_eq!(
            world.visits.try_recv().map(|slug| slug.to_string()),
            Ok("go-x".to_owned())
        );
    }

    #[tokio::test]
    async fn unknown_and_invalid_slugs_are_not_found() {
        let world = world();
        for uri in ["/nothing-here", "/NOT_VALID", "/links/nothing-here"] {
            let (status, _) = call(&world.router, "GET", uri, None).await;
            assert_eq!(status, StatusCode::NOT_FOUND, "{uri}");
        }
    }

    #[tokio::test]
    async fn shows_a_link() {
        let world = world();
        call(
            &world.router,
            "POST",
            "/links",
            Some(r#"{"target":"https://example.org","slug":"info"}"#),
        )
        .await;
        let (status, response) = call(&world.router, "GET", "/links/info", None).await;
        assert_eq!(status, StatusCode::OK);
        let view: LinkView = serde_json::from_value(json(response).await).unwrap();
        assert_eq!(
            view,
            LinkView {
                slug: "info".to_owned(),
                target: "https://example.org".to_owned(),
                visits: 0
            }
        );
    }
}
