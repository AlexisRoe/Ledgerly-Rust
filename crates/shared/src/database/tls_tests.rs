//! TLS to Postgres (plan M11.4), against a Postgres that accepts TLS connections only. Its certificate is issued
//! for `localhost` by a CA made up for the test; the CA is the `sslrootcert`.
//!
//! The certificates are made inside the container (the Postgres image has `openssl`), so the private keys
//! never touch this machine's disk and need no file permissions fixed.

// clippy.toml exempts `#[test]` functions only; the helpers of a test module are test code too.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::time::{Duration, Instant};

use secrecy::SecretString;
use sqlx::{Connection, PgConnection};

use super::connect_options;
use super::testing::{LABEL, docker, image_of};
use crate::config::{DatabaseConfig, DatabaseUrl};

/// Makes a CA and a server certificate for `localhost`, lets only TLS connections in, then starts Postgres.
const START: &str = r#"set -e
mkdir -p /certs && cd /certs
openssl req -x509 -newkey rsa:2048 -nodes -days 1 -subj /CN=ledgerly-test-ca -keyout ca.key -out ca.crt 2>/dev/null
openssl req -newkey rsa:2048 -nodes -subj /CN=localhost -keyout server.key -out server.csr 2>/dev/null
printf 'subjectAltName=DNS:localhost\n' > san.ext
openssl x509 -req -in server.csr -CA ca.crt -CAkey ca.key -CAcreateserial -days 1 -extfile san.ext -out server.crt 2>/dev/null
chown postgres:postgres server.key server.crt
chmod 600 server.key
printf 'local all all trust\nhostssl all all all scram-sha-256\n' > pg_hba.conf
exec docker-entrypoint.sh postgres -c ssl=on -c ssl_cert_file=/certs/server.crt -c ssl_key_file=/certs/server.key -c hba_file=/certs/pg_hba.conf"#;

/// Removes the container, also when the test fails.
struct Container(String);

impl Drop for Container {
    fn drop(&mut self) {
        let _ = docker(&["rm", "--force", "--volumes", &self.0]);
    }
}

struct TlsOnlyPostgres {
    container: Container,
    port: u16,
    root_cert: tempfile::TempPath,
}

impl TlsOnlyPostgres {
    async fn start() -> Self {
        let name = format!("ledgerly-tls-test-{}", uuid::Uuid::new_v4().simple());
        docker(&[
            "run",
            "--detach",
            "--rm",
            "--label",
            LABEL,
            "--name",
            &name,
            "--env",
            "POSTGRES_USER=ledgerly",
            "--env",
            "POSTGRES_PASSWORD=ledgerly",
            "--env",
            "POSTGRES_DB=ledgerly",
            "--publish",
            "127.0.0.1::5432",
            "--entrypoint",
            "sh",
            image_of("postgres").unwrap(),
            "-c",
            START,
        ])
        .unwrap();
        let container = Container(name);
        let published = docker(&["port", &container.0, "5432/tcp"]).unwrap();
        let port = published
            .lines()
            .next()
            .and_then(|line| line.rsplit_once(':'))
            .and_then(|(_, port)| port.trim().parse().ok())
            .unwrap();
        let root_cert = tempfile::NamedTempFile::new().unwrap().into_temp_path();
        let server = Self {
            container,
            port,
            root_cert,
        };
        server.wait_until_ready().await;
        let ca = root_cert_path(&server.root_cert);
        docker(&["cp", &format!("{}:/certs/ca.crt", server.container.0), &ca]).unwrap();
        server
    }

    /// Until the final server (not the one the image runs while it initialises) takes a TLS login.
    async fn wait_until_ready(&self) {
        let deadline = Instant::now() + Duration::from_secs(120);
        loop {
            if self.connect("localhost", "sslmode=require").await.is_ok() {
                return;
            }
            assert!(Instant::now() < deadline, "the TLS-only Postgres did not start");
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }

    async fn connect(&self, host: &str, parameters: &str) -> Result<PgConnection, sqlx::Error> {
        let url = DatabaseUrl::parse(&format!("jdbc:postgresql://{host}:{}/ledgerly?{parameters}", self.port)).unwrap();
        let config = DatabaseConfig {
            url,
            username: "ledgerly".to_owned(),
            password: SecretString::from("ledgerly"),
        };
        PgConnection::connect_with(&connect_options(&config)).await
    }

    fn verifying(&self, mode: &str) -> String {
        format!("sslmode={mode}&sslrootcert={}", root_cert_path(&self.root_cert))
    }
}

fn root_cert_path(path: &tempfile::TempPath) -> String {
    path.to_str().unwrap().to_owned()
}

async fn encrypted(connection: &mut PgConnection) -> bool {
    sqlx::query_scalar("select ssl from pg_stat_ssl where pid = pg_backend_pid()")
        .fetch_one(connection)
        .await
        .unwrap()
}

#[tokio::test]
async fn verify_full_connects_when_the_certificate_names_the_host_and_refuses_otherwise() {
    let server = TlsOnlyPostgres::start().await;

    let mut connection = server
        .connect("localhost", &server.verifying("verify-full"))
        .await
        .unwrap();
    assert!(encrypted(&mut connection).await);

    // The certificate is for `localhost`, not for 127.0.0.1.
    assert!(
        server
            .connect("127.0.0.1", &server.verifying("verify-full"))
            .await
            .is_err()
    );
}

/// Decided after M11.4: `verify-ca` checks the host name too, because the system's CA certificates are trusted
/// besides `sslrootcert`.
#[tokio::test]
async fn verify_ca_is_checked_like_verify_full() {
    let server = TlsOnlyPostgres::start().await;

    assert!(
        server
            .connect("localhost", &server.verifying("verify-ca"))
            .await
            .is_ok()
    );
    assert!(
        server
            .connect("127.0.0.1", &server.verifying("verify-ca"))
            .await
            .is_err()
    );
}

/// Without a root certificate the CA is unknown: a `verify-*` mode refuses it.
#[tokio::test]
async fn an_unknown_ca_is_refused_by_verify_full() {
    let server = TlsOnlyPostgres::start().await;

    assert!(server.connect("localhost", "sslmode=verify-full").await.is_err());
}

/// pgjdbc's `require` and `prefer` encrypt without checking the certificate; `disable` sends no TLS, which
/// this server refuses.
#[tokio::test]
async fn require_and_prefer_encrypt_without_checking_and_disable_is_refused() {
    let server = TlsOnlyPostgres::start().await;

    for mode in ["require", "prefer"] {
        let mut connection = server.connect("127.0.0.1", &format!("sslmode={mode}")).await.unwrap();
        assert!(encrypted(&mut connection).await, "{mode}");
    }
    assert!(server.connect("127.0.0.1", "sslmode=disable").await.is_err());
}
