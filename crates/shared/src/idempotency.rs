//! Remembers which resource an `Idempotency-Key` produced, for 24 hours.
//!
//! Java counterpart: `shared/idempotency/IdempotencyStore.java`, on the same table (V2) with the same rows,
//! except the request hash: Java hashes its `Command.toString()`, Rust its own canonical form (ADR 0005,
//! accepted exceptions). A replay returns the resource as it is now, not a copy of the first response.

use chrono::{DateTime, TimeDelta, Utc};
use sha2::{Digest, Sha256};
use sqlx::PgConnection;
use uuid::Uuid;

use crate::id::UserId;

/// How long a key is remembered. Java: `IdempotencyStore.TTL`.
pub const TTL: TimeDelta = TimeDelta::hours(24);

/// What was answered the first time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Lower-case hex SHA-256 of the request, see [`request_hash`].
    pub request_hash: String,
    pub response_status: i32,
    pub resource_id: Uuid,
}

/// Why a key could not be stored.
#[derive(Debug, thiserror::Error)]
pub enum SaveError {
    /// Another request stored the same key first; it may still be running. Java: `DuplicateKeyException`.
    #[error("the key is already taken")]
    Taken,
    #[error(transparent)]
    Database(sqlx::Error),
}

/// The unexpired entry for this user and key. Keys are per user.
///
/// # Errors
///
/// The database error, if the query fails.
pub async fn find(
    connection: &mut PgConnection,
    user: UserId,
    key: &str,
    now: DateTime<Utc>,
) -> Result<Option<Entry>, sqlx::Error> {
    let row: Option<(String, i32, Uuid)> = sqlx::query_as(
        "select request_hash, response_status, resource_id from idempotency_keys \
         where user_id = $1 and idem_key = $2 and expires_at > $3",
    )
    .bind(user.as_uuid())
    .bind(key)
    .bind(now)
    .fetch_optional(&mut *connection)
    .await?;
    Ok(row.map(|(request_hash, response_status, resource_id)| Entry {
        request_hash,
        response_status,
        resource_id,
    }))
}

/// Stores the entry until `now + 24 h`, after removing this user's expired row for the key.
///
/// A second request with the same key inserts the same primary key: Postgres makes it wait until the first
/// transaction ends and then refuses it, so at most one of them creates anything.
///
/// # Errors
///
/// [`SaveError::Taken`] if the key is stored already, or the database error.
pub async fn save(
    connection: &mut PgConnection,
    user: UserId,
    key: &str,
    entry: &Entry,
    now: DateTime<Utc>,
) -> Result<(), SaveError> {
    sqlx::query("delete from idempotency_keys where user_id = $1 and idem_key = $2 and expires_at <= $3")
        .bind(user.as_uuid())
        .bind(key)
        .bind(now)
        .execute(&mut *connection)
        .await
        .map_err(SaveError::Database)?;
    let inserted = sqlx::query(
        "insert into idempotency_keys \
           (user_id, idem_key, request_hash, response_status, resource_id, created_at, expires_at) \
         values ($1, $2, $3, $4, $5, $6, $7)",
    )
    .bind(user.as_uuid())
    .bind(key)
    .bind(&entry.request_hash)
    .bind(entry.response_status)
    .bind(entry.resource_id)
    .bind(now)
    .bind(now + TTL)
    .execute(&mut *connection)
    .await;
    match inserted {
        Ok(_) => Ok(()),
        Err(sqlx::Error::Database(error)) if error.is_unique_violation() => Err(SaveError::Taken),
        Err(error) => Err(SaveError::Database(error)),
    }
}

/// Lower-case hex SHA-256 of a request's canonical form: its parts joined by a separator that cannot occur in
/// them (`\u{0}`). The caller decides what the parts are and in which order.
#[must_use]
pub fn request_hash(parts: &[&str]) -> String {
    let mut hasher = Sha256::new();
    for (index, part) in parts.iter().enumerate() {
        if index > 0 {
            hasher.update([0_u8]);
        }
        hasher.update(part.as_bytes());
    }
    hasher.finalize().iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_hash_is_64_lower_hex_and_separates_its_parts() {
        let hash = request_hash(&["a", "bc"]);
        assert_eq!(hash.len(), 64);
        assert!(hash.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)));
        assert_ne!(hash, request_hash(&["ab", "c"]));
        assert_eq!(hash, request_hash(&["a", "bc"]));
    }
}
