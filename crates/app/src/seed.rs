//! Demo data for trying the API at once, in the `local` profile only.
//!
//! Java counterpart: `shared/seed/DevDataSeeder.java`; the values are in `docs/reference/demo-data.md`. It writes
//! SQL into the modules' tables directly, as Java does, bypassing the domain: the submitted, approved and
//! reimbursed expenses have no receipts, and no events are written. Unlike Java, everything is one transaction,
//! so a failed start leaves no half-made team behind.

use chrono::{DateTime, TimeDelta, Utc};
use ledgerly_shared::config::Profiles;
use rust_decimal::Decimal;
use secrecy::SecretString;
use sqlx::{PgConnection, PgPool};
use uuid::Uuid;

/// The shared password of the demo users. A published development value, `local` only.
const PASSWORD: &str = "correct-horse-battery";
/// Fixed, so that the Bruno collection and the docs can refer to it.
pub const TEAM: Uuid = Uuid::from_u128(0x0000_0000_0000_7000_8000_0000_0000_a001);

/// What the seeder did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Seeded {
    /// Not `local`, or `local` with `test`: nothing to do.
    NotLocal,
    /// `keycloak`: the identity provider owns the accounts.
    SkippedForKeycloak,
    /// The demo team exists already, so a restart adds no duplicates.
    AlreadyThere,
    Created,
}

/// Why seeding failed. The application does not start, as Java's does not when an `ApplicationRunner` throws.
#[derive(Debug, thiserror::Error)]
pub enum SeedError {
    #[error("Demo data could not be written: {0}")]
    Database(#[from] sqlx::Error),
    #[error("Demo data could not be written: the password could not be hashed")]
    Hashing,
}

/// Fills an empty database with the demo team, under `local` and not `test`. Java: `DevDataSeeder.run`.
///
/// # Errors
///
/// A [`SeedError`] if the database refuses a statement or hashing fails.
pub async fn demo_data(profiles: Profiles, pool: &PgPool, now: DateTime<Utc>) -> Result<Seeded, SeedError> {
    if !profiles.local || profiles.test {
        return Ok(Seeded::NotLocal);
    }
    if profiles.keycloak {
        tracing::info!("Demo data skipped: accounts are managed by the identity provider");
        return Ok(Seeded::SkippedForKeycloak);
    }
    let mut transaction = pool.begin().await?;
    let exists: bool = sqlx::query_scalar("select exists (select 1 from teams where id = $1)")
        .bind(TEAM)
        .fetch_one(&mut *transaction)
        .await?;
    if exists {
        return Ok(Seeded::AlreadyThere);
    }
    // Hashed once; all four users share it.
    let hash = tokio::task::spawn_blocking(|| ledgerly_identity::password_hash(&SecretString::from(PASSWORD)))
        .await
        .ok()
        .flatten()
        .ok_or(SeedError::Hashing)?;
    let connection = &mut *transaction;

    let alice = user(connection, "alice@example.com", &hash, true, now).await?;
    let bob = user(connection, "bob@example.com", &hash, true, now).await?;
    let carol = user(connection, "carol@example.com", &hash, true, now).await?;
    user(connection, "dave@example.com", &hash, false, now).await?;

    sqlx::query("insert into teams (id, name, created_at) values ($1, 'Acme Travel', $2)")
        .bind(TEAM)
        .bind(now)
        .execute(&mut *connection)
        .await?;
    for (member, role) in [(alice, "OWNER"), (bob, "APPROVER"), (carol, "MEMBER")] {
        sqlx::query("insert into memberships (team_id, user_id, role, joined_at) values ($1, $2, $3, $4)")
            .bind(TEAM)
            .bind(member)
            .bind(role)
            .bind(now)
            .execute(&mut *connection)
            .await?;
    }
    sqlx::query(
        "insert into invitations (id, team_id, email, role, status, invited_by, created_at, expires_at) \
         values ($1, $2, 'dave@example.com', 'MEMBER', 'PENDING', $3, $4, $5)",
    )
    .bind(Uuid::now_v7())
    .bind(TEAM)
    .bind(alice)
    .bind(now)
    .bind(now + TimeDelta::days(7))
    .execute(&mut *connection)
    .await?;

    let mut rejected = None;
    // Amounts in hundredths: 3840 is 38.40.
    for (claimant, amount, currency, category, days_ago, merchant, status) in [
        (carol, 3840, "EUR", "MEALS", 2, "Cafe Central", "DRAFT"),
        (carol, 21200, "EUR", "TRAVEL", 6, "Rail Europe", "SUBMITTED"),
        (carol, 4999, "EUR", "SOFTWARE", 9, "Editor Inc", "APPROVED"),
        (carol, 98000, "EUR", "OFFICE", 12, "Desk Depot", "REJECTED"),
        (carol, 7500, "EUR", "TRAVEL", 20, "City Taxi", "REIMBURSED"),
        (bob, 1990, "USD", "OTHER", 1, "Parking", "DRAFT"),
        (alice, 12000, "EUR", "MEALS", 4, "Team dinner", "SUBMITTED"),
    ] {
        let id = Uuid::now_v7();
        sqlx::query(
            "insert into expenses (id, team_id, claimant_id, amount, currency, category, incurred_on, merchant, \
                                   status, version, created_at, updated_at) \
             values ($1, $2, $3, $4, $5, $6, $7, $8, $9, 0, $10, $10)",
        )
        .bind(id)
        .bind(TEAM)
        .bind(claimant)
        .bind(Decimal::new(amount, 2))
        .bind(currency)
        .bind(category)
        .bind(now.date_naive() - TimeDelta::days(days_ago))
        .bind(merchant)
        .bind(status)
        .bind(now)
        .execute(&mut *connection)
        .await?;
        if status == "REJECTED" {
            rejected = Some(id);
        }
    }
    sqlx::query(
        "insert into expense_rejections (expense_id, position, rejected_by, reason) \
         values ($1, 0, $2, 'Over the office budget; please get a quote first.')",
    )
    .bind(rejected)
    .bind(bob)
    .execute(&mut *connection)
    .await?;
    transaction.commit().await?;

    tracing::info!(
        "Demo data created: team 'Acme Travel' with alice (OWNER), bob (APPROVER), carol (MEMBER), dave \
         (unverified, invited). Password for all: {PASSWORD}"
    );
    Ok(Seeded::Created)
}

/// Java's `user`: inserted unless the email is taken, then read back by email, so an address registered before
/// keeps its account.
async fn user(
    connection: &mut PgConnection,
    email: &str,
    hash: &str,
    verified: bool,
    now: DateTime<Utc>,
) -> sqlx::Result<Uuid> {
    sqlx::query(
        "insert into users (id, email, password_hash, verified, created_at) values ($1, $2, $3, $4, $5) \
         on conflict (email) do nothing",
    )
    .bind(Uuid::now_v7())
    .bind(email)
    .bind(hash)
    .bind(verified)
    .bind(now)
    .execute(&mut *connection)
    .await?;
    sqlx::query_scalar("select id from users where email = $1")
        .bind(email)
        .fetch_one(connection)
        .await
}
