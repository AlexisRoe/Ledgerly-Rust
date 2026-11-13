use crate::fixtures::{Fixtures, PASSWORD, User};
use crate::http::Client;

/// Where the system under test is, read from the environment `just parity` sets.
#[derive(Debug, Clone)]
pub struct Target {
    /// `PARITY_BASE_URL`: scheme, host and port of the API, such as `http://127.0.0.1:8080`.
    pub base_url: String,
    /// `PARITY_MANAGEMENT_URL`: the management port. Defaults to the API's host on port 8081.
    pub management_url: String,
    /// `PARITY_DATABASE_URL`: the database behind the API, as `postgres://user:password@host:port/database`.
    pub database_url: String,
    /// `PARITY_S3_ENDPOINT`: the object store behind the API. Defaults to SeaweedFS from `compose.yaml`.
    pub s3_endpoint: String,
    /// `PARITY_S3_BUCKET`: the bucket the API is configured with.
    pub s3_bucket: String,
}

fn variable(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|value| !value.trim().is_empty())
}

impl Target {
    /// Reads the target from the environment.
    ///
    /// # Panics
    ///
    /// Panics with instructions if `PARITY_BASE_URL` is not set: the suite needs a running system, which
    /// `just parity` provides.
    #[must_use]
    pub fn from_env() -> Self {
        let base_url = variable("PARITY_BASE_URL").unwrap_or_else(|| {
            panic!(
                "PARITY_BASE_URL is not set. The parity suite runs against a started app: use `just parity`, \
                 or `just parity http://host:port` for a deployment that is already running."
            )
        });
        let base_url = base_url.trim_end_matches('/').to_owned();
        let management_url = variable("PARITY_MANAGEMENT_URL").map_or_else(
            || management_default(&base_url),
            |url| url.trim_end_matches('/').to_owned(),
        );
        Self {
            base_url,
            management_url,
            database_url: variable("PARITY_DATABASE_URL")
                .unwrap_or_else(|| "postgres://ledgerly:ledgerly@127.0.0.1:5432/ledgerly".to_owned()),
            s3_endpoint: variable("PARITY_S3_ENDPOINT").unwrap_or_else(|| "http://127.0.0.1:8333".to_owned()),
            s3_bucket: variable("PARITY_S3_BUCKET").unwrap_or_else(|| "ledgerly-files".to_owned()),
        }
    }

    /// A client for the API port. Paths start at the root, so the API's are `/api/v1/...`.
    #[must_use]
    pub fn api(&self) -> Client {
        Client::new(&self.base_url)
    }

    /// A client for the management port (`/actuator/...`).
    #[must_use]
    pub fn management(&self) -> Client {
        Client::new(&self.management_url)
    }

    /// Logs a fixture user in over HTTP and returns the access token. This is how the suite gets tokens: it
    /// cannot mint them, as Java's tests do with `AccessTokenIssuer`.
    ///
    /// # Panics
    ///
    /// Panics if the login is refused.
    pub async fn token_for(&self, user: &User) -> String {
        let response = self
            .api()
            .post("/api/v1/auth/login")
            .json(&serde_json::json!({"email": user.email, "password": PASSWORD}))
            .send()
            .await;
        assert_eq!(
            response.status,
            200,
            "login of a fixture user failed: {}",
            response.text()
        );
        response.json()["accessToken"]
            .as_str()
            .expect("a login response has an accessToken")
            .to_owned()
    }

    /// Connects to the database for fixtures and assertions on stored data.
    pub async fn fixtures(&self) -> Fixtures {
        Fixtures::connect(self).await
    }
}

/// The same scheme and host with port 8081, Java's management port.
fn management_default(base_url: &str) -> String {
    let (scheme, rest) = base_url.split_once("://").unwrap_or(("http", base_url));
    let host = rest.rsplit_once(':').map_or(rest, |(host, _port)| host);
    format!("{scheme}://{host}:8081")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_management_port_defaults_to_8081_on_the_same_host() {
        assert_eq!(management_default("http://127.0.0.1:8080"), "http://127.0.0.1:8081");
        assert_eq!(
            management_default("https://ledgerly.example.com"),
            "https://ledgerly.example.com:8081"
        );
    }
}
