//! A database of its own for every integration test: Postgres 17 in Docker, with the schema built by the
//! pinned Flyway image from the vendored migrations (ADR 0006), exactly as `compose.yaml` builds it.
//!
//! Starting Postgres and Flyway's JVM for every test would take seconds each, and nextest runs every test in
//! its own process. So the expensive parts are shared and the cheap part is per test:
//!
//! - **One server.** A Postgres container named after its image digest, shared by every test process of a run.
//!   It removes itself 30 s after the last client disconnected (`--rm`, data on a tmpfs), and `just test`
//!   removes it as soon as the run ends.
//! - **One migrated template per set of migrations.** Flyway migrates a database once; it is then renamed to
//!   `ledgerly_template_<hash of the checksums>`. Re-vendored migrations get a new template.
//! - **One database per test.** `CREATE DATABASE … TEMPLATE …` copies the template in a few milliseconds. Tests
//!   therefore never see each other's rows, and may commit, lock and run concurrent transactions like the app.
//!
//! Who creates the template and the copies is serialised with a Postgres advisory lock, because many test
//! processes start at once. If one run follows another within 30 s, it reuses the container; databases older
//! than an hour are dropped.
//!
//! Docker is driven through its command line, like the parity runner does, not through `testcontainers`: that
//! crate depends on `astral-tokio-tar`, which is unmaintained (RUSTSEC-2026-0440) and so refused by
//! `cargo deny`. The few commands needed here do not justify a Docker client library.
//!
//! Only compiled with the `testing` feature, so none of it reaches the `ledgerly` binary.

use std::fmt;
use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use secrecy::SecretString;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::{AssertSqlSafe, Connection, PgConnection, PgPool};

use super::migrations::VendoredMigration;
use crate::config::{DatabaseConfig, DatabaseUrl};

/// The development credentials of `compose.yaml`; the container is reachable only from this machine's Docker.
const USER: &str = "ledgerly";
const PASSWORD: &str = "ledgerly";
/// Every container the tests start carries it; `just test-db-down` removes them by it.
const LABEL: &str = "org.ledgerly.purpose=integration-tests";
/// Any constant works, as long as nothing else in the test server takes the same advisory lock.
const LOCK: i64 = 0x6c65_6467_6572_6c79; // "ledgerly"
const STALE_AFTER: Duration = Duration::from_secs(60 * 60);
const COMPOSE: &str = include_str!("../../../../compose.yaml");

/// Why no test database could be provided. Tests `unwrap` it; the message says what to look at.
#[derive(Debug, thiserror::Error)]
pub enum TestDatabaseError {
    #[error("`docker {command}` failed: {reason}. Is Docker running?")]
    Docker { command: String, reason: String },
    #[error("Postgres: {0}")]
    Postgres(#[from] sqlx::Error),
    #[error("{0}")]
    Setup(String),
}

/// A fresh, migrated database that only this test uses.
pub struct TestDatabase {
    pool: PgPool,
    config: DatabaseConfig,
}

impl fmt::Debug for TestDatabase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TestDatabase")
            .field("database", &self.config.url.database())
            .finish_non_exhaustive()
    }
}

impl TestDatabase {
    /// Copies the migrated template into a new database and connects to it.
    ///
    /// # Errors
    ///
    /// If Docker or Postgres cannot be reached, or Flyway fails.
    pub async fn new() -> Result<Self, TestDatabaseError> {
        let server = Server::start().await?;
        let mut admin = server.connect("postgres").await?;
        sqlx::query("select pg_advisory_lock($1)")
            .bind(LOCK)
            .execute(&mut admin)
            .await?;
        let created = async {
            let template = server.ensure_template(&mut admin).await?;
            drop_stale_databases(&mut admin).await?;
            let name = fresh_name()?;
            ddl(&mut admin, format!(r#"create database "{name}" template "{template}""#)).await?;
            Ok::<_, TestDatabaseError>(name)
        }
        .await;
        // Unlocks also when the creation failed; the lock would otherwise end only with the connection.
        sqlx::query("select pg_advisory_unlock($1)")
            .bind(LOCK)
            .execute(&mut admin)
            .await?;
        admin.close().await?;
        let name = created?;

        let config = server.config(&name)?;
        let pool = PgPoolOptions::new()
            .max_connections(5)
            .connect_with(super::connect_options(&config))
            .await?;
        Ok(Self { pool, config })
    }

    /// A pool on this test's database.
    #[must_use]
    pub const fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// The settings that point the app at this test's database.
    #[must_use]
    pub const fn config(&self) -> &DatabaseConfig {
        &self.config
    }

    /// `SPRING_DATASOURCE_URL` for this test's database, to start the `ledgerly` binary against it. The
    /// username and password are [`TestDatabase::USERNAME`] and [`TestDatabase::PASSWORD`].
    #[must_use]
    pub fn jdbc_url(&self) -> String {
        let url = &self.config.url;
        format!("jdbc:postgresql://{}:{}/{}", url.host(), url.port(), url.database())
    }

    pub const USERNAME: &str = USER;
    pub const PASSWORD: &str = PASSWORD;
}

/// The shared Postgres container, reached from the host on a published port and from Flyway's container
/// through Postgres' own network namespace (`--network container:<name>`), so no Docker network is needed.
struct Server {
    name: String,
    host: String,
    port: u16,
}

impl Server {
    /// Starts the shared container unless it runs already, and finds the port Docker published it on.
    ///
    /// The container removes itself (see [`WATCHDOG`]), so a test may find it shutting down; then it waits for
    /// it to be gone and starts a new one.
    async fn start() -> Result<Self, TestDatabaseError> {
        let image = image_of("postgres")?;
        let digest = image.rsplit_once(':').map_or(image, |(_, digest)| digest);
        let name = format!("ledgerly-test-postgres-{}", digest.get(..12).unwrap_or(digest));

        let mut last_error = None;
        for _ in 0..60 {
            match running(&name) {
                // Whoever loses the race to create it uses the winner's, on the next round.
                None => {
                    if let Err(error) = create(&name, image) {
                        last_error = Some(error);
                    }
                }
                Some(true) => match Self::attach(&name).await {
                    Ok(server) => return Ok(server),
                    Err(error) => last_error = Some(error),
                },
                // Shutting down or being removed.
                Some(false) => {}
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        Err(last_error.unwrap_or_else(|| TestDatabaseError::Setup(format!("{name} never came up"))))
    }

    async fn attach(name: &str) -> Result<Self, TestDatabaseError> {
        // `127.0.0.1:55000`
        let published = docker(&["port", name, "5432/tcp"])?;
        let port = published
            .lines()
            .find_map(|line| line.strip_prefix("127.0.0.1:"))
            .and_then(|port| port.trim().parse().ok())
            .ok_or_else(|| TestDatabaseError::Setup(format!("no port published for {name}: {published}")))?;
        let server = Self {
            name: name.to_owned(),
            host: "127.0.0.1".to_owned(),
            port,
        };
        server.wait_until_accepting().await?;
        Ok(server)
    }

    /// A container just created, or one reused while it still starts, refuses connections for a moment.
    async fn wait_until_accepting(&self) -> Result<(), TestDatabaseError> {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
        loop {
            match self.connect("postgres").await {
                Ok(connection) => return Ok(connection.close().await?),
                Err(error) if tokio::time::Instant::now() > deadline => return Err(error.into()),
                Err(_) => tokio::time::sleep(Duration::from_millis(200)).await,
            }
        }
    }

    async fn connect(&self, database: &str) -> Result<PgConnection, sqlx::Error> {
        PgConnection::connect_with(
            &PgConnectOptions::new_without_pgpass()
                .host(&self.host)
                .port(self.port)
                .username(USER)
                .password(PASSWORD)
                .database(database),
        )
        .await
    }

    fn config(&self, database: &str) -> Result<DatabaseConfig, TestDatabaseError> {
        let url = DatabaseUrl::parse(&format!("postgres://{}:{}/{database}", self.host, self.port))
            .map_err(|error| TestDatabaseError::Setup(format!("test database URL {error}")))?;
        Ok(DatabaseConfig {
            url,
            username: USER.to_owned(),
            password: SecretString::from(PASSWORD),
        })
    }

    /// The template for the vendored migrations, migrated by Flyway if it does not exist yet. The caller holds
    /// the advisory lock.
    async fn ensure_template(&self, admin: &mut PgConnection) -> Result<String, TestDatabaseError> {
        let template = template_name();
        if database_exists(admin, &template).await? {
            return Ok(template);
        }
        // A database is only renamed to the template's name once Flyway succeeded, so a run that was
        // interrupted half-way leaves this one behind, never a broken template.
        let building = format!("{template}_building");
        ddl(admin, format!(r#"drop database if exists "{building}" with (force)"#)).await?;
        ddl(admin, format!(r#"create database "{building}""#)).await?;
        self.migrate(&building)?;
        ddl(admin, format!(r#"alter database "{building}" rename to "{template}""#)).await?;
        Ok(template)
    }

    /// Runs the Flyway image of `compose.yaml` against `database`, with the same settings as its `flyway`
    /// service, and waits for it to finish.
    fn migrate(&self, database: &str) -> Result<(), TestDatabaseError> {
        let migrations = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../vendor/ledgerly-java/src/main/resources/db/migration"
        );
        let migrations = std::fs::canonicalize(migrations)
            .map_err(|error| TestDatabaseError::Setup(format!("vendored migrations not found: {error}")))?;
        let migrations = migrations
            .to_str()
            .ok_or_else(|| TestDatabaseError::Setup("the path of the migrations is not UTF-8".to_owned()))?;
        docker(&[
            "run",
            "--rm",
            "--network",
            &format!("container:{}", self.name),
            "--volume",
            &format!("{migrations}:/flyway/sql:ro"),
            "--env",
            &format!("FLYWAY_URL=jdbc:postgresql://localhost:5432/{database}"),
            "--env",
            &format!("FLYWAY_USER={USER}"),
            "--env",
            &format!("FLYWAY_PASSWORD={PASSWORD}"),
            "--env",
            "REDGATE_DISABLE_TELEMETRY=true",
            image_of("flyway")?,
            "migrate",
        ])
        .map(|_| ())
    }
}

/// Inside the container: runs Postgres, and stops it once no client has been connected for 30 s (2 minutes
/// before the first test migrated the template). With `--rm` and the data on a tmpfs, the container then disappears without a trace,
/// whether the tests ran through `just test` or a bare `cargo nextest run`.
const WATCHDOG: &str = r#"
docker-entrypoint.sh postgres & server=$!
trap 'kill -INT "$server"' TERM INT
idle=0
while kill -0 "$server" 2>/dev/null; do
  sleep 2
  state=$(psql -U "$POSTGRES_USER" -d postgres -Atc \
    "select (select count(*) from pg_stat_activity where backend_type = 'client backend' and pid <> pg_backend_pid())
         || ' ' || (select count(*) from pg_database where datname like 'ledgerly\_template\_%')" \
    2>/dev/null) || continue
  read -r clients used <<< "$state"
  # Used once the migrated template exists; until then a test is still on its way.
  if [ "$used" -gt 0 ]; then limit=30; else limit=120; fi
  if [ "$clients" -gt 0 ]; then idle=0; else idle=$((idle + 2)); fi
  if [ "$idle" -ge "$limit" ]; then kill -INT "$server"; fi
done
wait "$server"
"#;

/// The container's state: `None` if there is none, otherwise whether it runs.
fn running(name: &str) -> Option<bool> {
    docker(&["container", "inspect", "--format", "{{.State.Running}}", name])
        .ok()
        .map(|state| state.trim() == "true")
}

fn create(name: &str, image: &str) -> Result<(), TestDatabaseError> {
    docker(&[
        "run",
        "--detach",
        "--rm",
        "--name",
        name,
        // On 127.0.0.1 only: with the well-known password, every interface would open it to the network.
        "--publish",
        "127.0.0.1::5432",
        // Nothing is written to disk, and nothing is left behind.
        "--tmpfs",
        "/var/lib/postgresql/data",
        "--env",
        &format!("POSTGRES_USER={USER}"),
        "--env",
        &format!("POSTGRES_PASSWORD={PASSWORD}"),
        "--env",
        &format!("POSTGRES_DB={USER}"),
        "--label",
        LABEL,
        "--entrypoint",
        "bash",
        image,
        "-c",
        WATCHDOG,
    ])
    .map(|_| ())
}

/// Runs `docker` with `args` and returns its standard output.
///
/// It blocks the calling thread. That is acceptable in test setup: each test process sets up one database,
/// before the test itself runs.
fn docker(args: &[&str]) -> Result<String, TestDatabaseError> {
    let failed = |reason: String| TestDatabaseError::Docker {
        command: args.first().copied().unwrap_or_default().to_owned(),
        reason,
    };
    let output = Command::new("docker")
        .args(args)
        .output()
        .map_err(|error| failed(error.to_string()))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        Err(failed(String::from_utf8_lossy(&output.stderr).trim().to_owned()))
    }
}

/// `ledgerly_template_<8 hex digits>`, from the versions and checksums of the vendored migrations.
fn template_name() -> String {
    let fingerprint: String = VendoredMigration::all()
        .iter()
        .map(|migration| format!("{}:{};", migration.version, migration.checksum()))
        .collect();
    let hash = super::flyway_checksum(&fingerprint);
    format!("ledgerly_template_{:08x}", u32::from_ne_bytes(hash.to_ne_bytes()))
}

/// `test_<unix seconds>_<random>`: the time in the name is what [`drop_stale_databases`] goes by.
fn fresh_name() -> Result<String, TestDatabaseError> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| TestDatabaseError::Setup(error.to_string()))?;
    Ok(format!("test_{}_{}", now.as_secs(), uuid::Uuid::new_v4().simple()))
}

/// Runs a statement whose identifiers this module made up itself (database names are never user input), which
/// is why it may be built with `format!`. Postgres cannot take identifiers as bind parameters.
async fn ddl(admin: &mut PgConnection, statement: String) -> Result<(), sqlx::Error> {
    sqlx::raw_sql(AssertSqlSafe(statement)).execute(admin).await.map(|_| ())
}

async fn database_exists(admin: &mut PgConnection, name: &str) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar::<_, bool>("select exists (select from pg_database where datname = $1)")
        .bind(name)
        .fetch_one(admin)
        .await
}

/// Drops the test databases of runs more than an hour ago, so the reused server does not fill up.
async fn drop_stale_databases(admin: &mut PgConnection) -> Result<(), TestDatabaseError> {
    let names: Vec<String> = sqlx::query_scalar("select datname from pg_database where datname like 'test\\_%'")
        .fetch_all(&mut *admin)
        .await?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| TestDatabaseError::Setup(error.to_string()))?;
    for name in names {
        let created = name
            .strip_prefix("test_")
            .and_then(|rest| rest.split_once('_'))
            .and_then(|(seconds, _)| seconds.parse::<u64>().ok());
        if created.is_some_and(|seconds| now.as_secs().saturating_sub(seconds) > STALE_AFTER.as_secs()) {
            ddl(admin, format!(r#"drop database if exists "{name}" with (force)"#)).await?;
        }
    }
    Ok(())
}

/// The `image:` of a service in `compose.yaml`, so that the tests use the very images the dev loop uses, pinned
/// by the same digests, and Renovate updates both at once.
fn image_of(service: &str) -> Result<&'static str, TestDatabaseError> {
    let header = format!("  {service}:");
    COMPOSE
        .lines()
        .skip_while(|line| *line != header)
        .skip(1)
        .take_while(|line| line.starts_with("    ") || line.trim().is_empty())
        .find_map(|line| line.trim().strip_prefix("image: "))
        .ok_or_else(|| TestDatabaseError::Setup(format!("compose.yaml has no image for the service {service}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn images_come_from_compose_pinned_by_digest() {
        let postgres = image_of("postgres").unwrap();
        assert!(postgres.starts_with("postgres:17@sha256:"), "{postgres}");
        let flyway = image_of("flyway").unwrap();
        assert!(
            flyway.starts_with("flyway/flyway:") && flyway.contains("@sha256:"),
            "{flyway}"
        );
        assert!(image_of("nonexistent").is_err());
    }

    #[test]
    fn the_template_name_follows_the_migrations() {
        let name = template_name();
        assert_eq!(name.len(), "ledgerly_template_".len() + 8);
        assert_eq!(name, template_name());
    }
}
