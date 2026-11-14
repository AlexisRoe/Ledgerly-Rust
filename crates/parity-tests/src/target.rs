use crate::fixtures::Fixtures;
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
    /// `PARITY_KEYCLOAK_URL`: the Keycloak of `compose.yaml`, as the tests reach it.
    pub keycloak_url: String,
    /// `PARITY_APP_CONTAINER`: the name of the container the app runs in, when `just parity` started it. Its
    /// stdout is the app's log.
    pub app_container: Option<String>,
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
            keycloak_url: variable("PARITY_KEYCLOAK_URL").unwrap_or_else(|| "http://127.0.0.1:8180".to_owned()),
            app_container: variable("PARITY_APP_CONTAINER"),
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

    /// `url` (a presigned link the API handed out) with its scheme and host replaced by `PARITY_S3_ENDPOINT`.
    ///
    /// The app names the object store as it reaches it, which inside Compose is `http://seaweedfs:8333`; the
    /// tests reach the same store through the published port. The development store does not check
    /// signatures, so the changed host does not invalidate the link.
    #[must_use]
    pub fn object_store_url(&self, url: &str) -> String {
        let after_scheme = url.split_once("://").map_or(url, |(_, rest)| rest);
        let path = after_scheme.find('/').map_or("/", |at| &after_scheme[at..]);
        format!("{}{path}", self.s3_endpoint.trim_end_matches('/'))
    }

    /// Everything the app has logged so far (stdout and stderr of its container).
    ///
    /// # Panics
    ///
    /// Panics if the app was not started by `just parity`, or if Docker cannot be asked.
    #[must_use]
    pub fn app_logs(&self) -> String {
        let container = self.app_container.as_deref().unwrap_or_else(|| {
            panic!("PARITY_APP_CONTAINER is not set: the logs can only be read when `just parity` started the app")
        });
        let output = std::process::Command::new("docker")
            .args(["logs", container])
            .output()
            .unwrap_or_else(|error| panic!("docker logs {container}: {error}"));
        assert!(output.status.success(), "docker logs {container} failed");
        format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
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
    fn a_presigned_link_is_pointed_at_the_published_object_store() {
        let target = Target {
            base_url: "http://127.0.0.1:8080".to_owned(),
            management_url: "http://127.0.0.1:8081".to_owned(),
            database_url: String::new(),
            s3_endpoint: "http://127.0.0.1:8333/".to_owned(),
            s3_bucket: "ledgerly-files".to_owned(),
            keycloak_url: String::new(),
            app_container: None,
        };

        assert_eq!(
            target.object_store_url("http://seaweedfs:8333/ledgerly-files/receipts/x?X-Amz-Expires=300&a=b"),
            "http://127.0.0.1:8333/ledgerly-files/receipts/x?X-Amz-Expires=300&a=b"
        );
    }

    #[test]
    fn the_management_port_defaults_to_8081_on_the_same_host() {
        assert_eq!(management_default("http://127.0.0.1:8080"), "http://127.0.0.1:8081");
        assert_eq!(
            management_default("https://ledgerly.example.com"),
            "https://ledgerly.example.com:8081"
        );
    }
}
