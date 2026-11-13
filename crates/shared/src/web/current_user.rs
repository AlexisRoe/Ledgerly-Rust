//! Who is calling.
//!
//! Java counterpart: the `CurrentUser` interface (`shared/security/CurrentUser.java`) and its one implementation,
//! `JwtCurrentUser`, which reads Spring Security's context. Here the authentication layer (M5) puts a
//! [`CurrentUser`] into the request's extensions once it has checked the token, and handlers take it as an
//! extractor. Until then nothing puts it there, so every endpoint that needs a caller answers 401: there is no
//! debug header or other way to claim an identity (ADR 0011). In-process tests insert it into the request they
//! build.

use axum::extract::FromRequestParts;
use axum::http::request::Parts;

use super::problem::Problem;
use crate::id::UserId;

/// The authenticated caller of the current request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CurrentUser(UserId);

impl CurrentUser {
    /// The caller, once something has proven who they are. Only the authentication layer and tests call this.
    #[must_use]
    pub const fn new(id: UserId) -> Self {
        Self(id)
    }

    #[must_use]
    pub const fn id(self) -> UserId {
        self.0
    }
}

/// Without an authenticated caller: 401 `UNAUTHENTICATED` with `WWW-Authenticate: Bearer`, as Java's
/// `Unauthenticated` exception and `ProblemSecurityHandler` answer.
impl<S: Send + Sync> FromRequestParts<S> for CurrentUser {
    type Rejection = Problem;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Problem> {
        parts
            .extensions
            .get::<Self>()
            .copied()
            .ok_or_else(Problem::unauthenticated)
    }
}

#[cfg(test)]
mod tests {
    use axum::http::{Request, StatusCode};

    use super::*;

    async fn extract(request: Request<()>) -> Result<CurrentUser, Problem> {
        let (mut parts, ()) = request.into_parts();
        CurrentUser::from_request_parts(&mut parts, &()).await
    }

    #[tokio::test]
    async fn the_caller_is_whoever_the_authentication_layer_put_into_the_request() {
        let caller = CurrentUser::new(UserId::new_v7());
        let mut request = Request::new(());
        request.extensions_mut().insert(caller);

        assert_eq!(extract(request).await, Ok(caller));
    }

    #[tokio::test]
    async fn nobody_is_401_and_no_header_can_claim_an_identity() {
        let request = Request::builder()
            .header("X-Debug-User", UserId::new_v7().to_string())
            .header("Authorization", "Bearer anything")
            .body(())
            .unwrap();

        let problem = extract(request).await.unwrap_err();

        assert_eq!(problem.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(problem.code().code(), "UNAUTHENTICATED");
        assert_eq!(problem.detail(), Some("Authentication required"));
    }
}
