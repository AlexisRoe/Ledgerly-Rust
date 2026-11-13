use std::sync::Arc;
use std::time::Duration;

use axum::body::Body as HttpBody;
use axum::http::{Request, StatusCode, header};
use http_body_util::BodyExt;
use ledgerly_shared::clock::{Clock, SystemClock};
use serde_json::Value;
use tower::ServiceExt;

use super::*;
use crate::access_token::SigningKeys;

#[tokio::test]
async fn the_jwks_holds_only_the_public_key_named_by_its_kid() {
    let keys = Arc::new(SigningKeys::from_config(None, None, true).unwrap());
    let clock: Arc<dyn Clock> = Arc::new(SystemClock);
    let tokens = AccessTokens::new(keys.clone(), Duration::from_secs(600), clock);
    let request = Request::get(JWKS).body(HttpBody::empty()).unwrap();

    let response = routes(&tokens).into_service().oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CONTENT_TYPE], "application/json");
    let body: Value = serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    let all = body["keys"].as_array().unwrap();
    assert_eq!(all.len(), 1);
    let names: Vec<&str> = all[0].as_object().unwrap().keys().map(String::as_str).collect();
    assert_eq!(names, ["crv", "kid", "kty", "x", "y"]);
    assert_eq!(all[0]["kid"], keys.kid());
    assert_eq!(
        (all[0]["kty"].as_str(), all[0]["crv"].as_str()),
        (Some("EC"), Some("P-256"))
    );
}
