//! What a test starts with: the target, its database, and users that are signed in.

use std::time::{Duration, Instant};

use chrono::Utc;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::Target;
use crate::fixtures::{Fixtures, PASSWORD, Team, User};
use crate::http::{Client, Response};

/// The server prefix of the API (`servers` in `api/openapi.yaml`).
pub const V1: &str = "/api/v1";

/// A user with an access token.
///
/// Java's tests mint tokens with `AccessTokenIssuer`. From outside the process a token can only come from
/// logging in, so every signed-in fixture user costs one `POST /auth/login`.
#[derive(Debug, Clone)]
pub struct Session {
    pub user: User,
    pub token: String,
}

impl Session {
    #[must_use]
    pub const fn id(&self) -> Uuid {
        self.user.id
    }

    #[must_use]
    pub fn email(&self) -> &str {
        &self.user.email
    }
}

/// `OwaspTestBase.Team`: a team with one signed-in user per role, plus a second MEMBER.
#[derive(Debug, Clone)]
pub struct SignedInTeam {
    pub id: Uuid,
    pub owner: Session,
    pub approver: Session,
    pub member: Session,
    pub colleague: Session,
}

impl SignedInTeam {
    /// `/api/v1/teams/{id}`
    #[must_use]
    pub fn path(&self) -> String {
        format!("{V1}/teams/{}", self.id)
    }

    /// `/api/v1/teams/{id}/expenses/{expense}`
    #[must_use]
    pub fn expense_path(&self, expense: Uuid) -> String {
        format!("{V1}/teams/{}/expenses/{expense}", self.id)
    }
}

/// The system under test and its database.
#[derive(Debug, Clone)]
pub struct World {
    pub target: Target,
    pub fixtures: Fixtures,
    /// The client address this test claims in `X-Forwarded-For`.
    ///
    /// Java's tests mint tokens in-process; here every fixture user logs in over HTTP, and the login endpoint
    /// is limited per client address. In the run group with a low limit the app is started with
    /// `SERVER_FORWARD_HEADERS_STRATEGY=native`, which makes it take the address from this header, so each test
    /// has its own allowance, as each Java test has its own Spring context. Elsewhere the header is ignored.
    pub client_address: String,
}

impl World {
    /// Reads the target from the environment and connects to its database.
    pub async fn start() -> Self {
        let target = Target::from_env();
        let fixtures = target.fixtures().await;
        // A public-looking address: a proxy-aware server skips private ones when it reads the header.
        let random = Uuid::new_v4().into_bytes();
        let client_address = format!(
            "{}.{}.{}.{}",
            20 + random[0] % 80,
            random[1],
            random[2],
            1 + random[3] % 254
        );
        Self {
            target,
            fixtures,
            client_address,
        }
    }

    /// A client without a token.
    #[must_use]
    pub fn anonymous(&self) -> Client {
        self.target
            .api()
            .with_default_header("X-Forwarded-For", &self.client_address)
    }

    /// A client that sends `session`'s token.
    #[must_use]
    pub fn as_user(&self, session: &Session) -> Client {
        self.anonymous().with_bearer(&session.token)
    }

    /// A client that sends `token` as the bearer token, whatever it is: forged, expired, garbage.
    #[must_use]
    pub fn with_token(&self, token: &str) -> Client {
        self.anonymous().with_bearer(token)
    }

    /// The client for the management port.
    #[must_use]
    pub fn management(&self) -> Client {
        self.target.management()
    }

    /// `POST /auth/login` with any credentials.
    pub async fn login(&self, email: &str, password: &str) -> Response {
        self.anonymous()
            .post(&format!("{V1}/auth/login"))
            .json(&json!({"email": email, "password": password}))
            .send()
            .await
    }

    /// `POST /auth/register`.
    pub async fn register(&self, email: &str, password: &str) -> Response {
        self.anonymous()
            .post(&format!("{V1}/auth/register"))
            .json(&json!({"email": email, "password": password}))
            .send()
            .await
    }

    /// `POST /auth/verify`.
    pub async fn verify(&self, token: &str) -> Response {
        self.anonymous()
            .post(&format!("{V1}/auth/verify"))
            .json(&json!({"token": token}))
            .send()
            .await
    }

    /// `POST /auth/refresh` with the CSRF header and the refresh cookie, as a browser on the same site sends
    /// them.
    pub async fn refresh(&self, cookie: &str) -> Response {
        self.anonymous()
            .post(&format!("{V1}/auth/refresh"))
            .header("X-Requested-With", "ledgerly")
            .header("Cookie", &format!("refresh_token={cookie}"))
            .send()
            .await
    }

    /// `POST /auth/logout` with the CSRF header and the refresh cookie.
    pub async fn logout(&self, cookie: &str) -> Response {
        self.anonymous()
            .post(&format!("{V1}/auth/logout"))
            .header("X-Requested-With", "ledgerly")
            .header("Cookie", &format!("refresh_token={cookie}"))
            .send()
            .await
    }

    /// The id of the user registered with `email` (stored lower-case), read from the database.
    pub async fn user_id_of(&self, email: &str) -> Uuid {
        sqlx::query_scalar("select id from users where email = $1")
            .bind(email.to_lowercase())
            .fetch_one(self.fixtures.pool())
            .await
            .unwrap_or_else(|error| panic!("no user with the email {email}: {error}"))
    }

    /// Registers a fresh address over HTTP and verifies it. Returns the user, whose password is `password`.
    ///
    /// Java reads the verification token from a test mailbox. Here a second verification is written for the
    /// user with a token the test knows, and that one is redeemed over HTTP (ADR 0011).
    pub async fn register_and_verify(&self, password: &str) -> User {
        let email = format!("owasp-{}@example.com", Uuid::new_v4());
        assert_eq!(self.register(&email, password).await.status, 202);
        let user = User {
            id: self.user_id_of(&email).await,
            email,
        };
        let token = self.fixtures.verification_token(&user).await;
        assert_eq!(self.verify(&token).await.status, 204);
        user
    }

    /// Logs a fixture user in (password [`PASSWORD`]) and returns the session.
    ///
    /// # Panics
    ///
    /// Panics if the login is refused.
    pub async fn sign_in(&self, user: &User) -> Session {
        let response = self.login(&user.email, PASSWORD).await;
        assert_eq!(
            response.status,
            200,
            "login of a fixture user failed: {}",
            response.text()
        );
        let token = response.json()["accessToken"]
            .as_str()
            .expect("a login response has an accessToken")
            .to_owned();
        Session {
            user: user.clone(),
            token,
        }
    }

    /// A verified, signed-in user who is in no team.
    pub async fn user(&self) -> Session {
        let user = self.fixtures.user().await;
        self.sign_in(&user).await
    }

    /// A signed-in user whose email is not verified.
    pub async fn unverified_user(&self) -> Session {
        let user = self.fixtures.unverified_user().await;
        self.sign_in(&user).await
    }

    /// `OwaspTestBase.newTeam()`: a team with an OWNER, an APPROVER, a MEMBER and a second MEMBER, all signed in.
    pub async fn team(&self) -> SignedInTeam {
        let Team {
            id,
            owner,
            approver,
            member,
            colleague,
        } = self.fixtures.team_with_roles().await;
        SignedInTeam {
            id,
            owner: self.sign_in(&owner).await,
            approver: self.sign_in(&approver).await,
            member: self.sign_in(&member).await,
            colleague: self.sign_in(&colleague).await,
        }
    }

    /// `OwaspTestBase.expense()`: an expense of `claimant` in `status`, written straight to the database.
    pub async fn expense(&self, team: &SignedInTeam, claimant: &Session, status: &str) -> Uuid {
        self.fixtures.expense(team.id, &claimant.user, status).await
    }

    /// Creates a draft over HTTP as `claimant` and returns the response (201 with `Location` and `ETag`).
    pub async fn create_expense(&self, team: &SignedInTeam, claimant: &Session, body: &Value) -> Response {
        self.as_user(claimant)
            .post(&format!("{}/expenses", team.path()))
            .json(body)
            .send()
            .await
    }
}

/// Today's date in UTC, as the API writes dates: `2026-11-13`.
#[must_use]
pub fn today() -> String {
    Utc::now().date_naive().to_string()
}

/// `OwaspTestBase.expenseJson()`: a valid create-expense body in EUR, category MEALS, incurred today.
#[must_use]
pub fn expense_json(amount: &str, merchant: &str) -> Value {
    json!({
        "money": {"amount": amount, "currency": "EUR"},
        "category": "MEALS",
        "incurredOn": today(),
        "merchant": merchant,
    })
}

/// Calls `check` every 250 ms until it returns `Some`, for at most `timeout`. For what the API does after it
/// has answered: the receipt scan, an import, the audit trail.
///
/// # Panics
///
/// Panics with `what` if the time runs out.
pub async fn eventually<T, F>(what: &str, timeout: Duration, mut check: impl FnMut() -> F) -> T
where
    F: Future<Output = Option<T>>,
{
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(value) = check().await {
            return value;
        }
        assert!(
            Instant::now() < deadline,
            "timed out after {timeout:?} waiting for {what}"
        );
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_expense_body_is_javas_expense_json() {
        let body = expense_json("12.50", "Cafe");

        assert_eq!(body["money"], json!({"amount": "12.50", "currency": "EUR"}));
        assert_eq!(body["category"], "MEALS");
        assert_eq!(body["merchant"], "Cafe");
        assert_eq!(body["incurredOn"], today());
        assert_eq!(today().len(), 10);
    }

    #[tokio::test]
    async fn eventually_returns_as_soon_as_the_check_succeeds() {
        let mut calls = 0;
        let value = eventually("the third call", Duration::from_secs(5), || {
            calls += 1;
            let done = calls >= 3;
            async move { done.then_some("ready") }
        })
        .await;

        assert_eq!((value, calls), ("ready", 3));
    }
}
