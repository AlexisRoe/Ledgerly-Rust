//! The tables of Ledgerly-Java's V3, V9 and V10 migrations: `users`, `email_verifications`, `refresh_tokens`.
//!
//! Java counterpart: `identity/internal/infrastructure/{UserRepository,EmailVerificationRepository,
//! RefreshTokenRepository}.java` and the JPA mappings in `internal/domain/`.
//!
//! None of these is team data. Tokens are found only by the hash of the raw token, and the lookups that change
//! a token lock its row (`for update`, Java's `PESSIMISTIC_WRITE`), so two requests with the same token are
//! serialised.
//!
//! The functions take a `&mut PgConnection`, which a pool connection and an open transaction both deref to, so
//! the caller decides the transaction boundary.

use chrono::{DateTime, Utc};
use ledgerly_shared::id::UserId;
use sqlx::PgConnection;
use uuid::Uuid;

use crate::opaque_token::TokenHash;
use crate::password::PasswordHash;

/// An account as login needs it.
#[derive(Debug, Clone)]
pub(crate) struct User {
    pub id: UserId,
    /// `None` for an account managed by an identity provider (V10).
    pub password_hash: Option<PasswordHash>,
}

/// The account with this normalised email. Java: `UserRepository.findByEmail`.
pub(crate) async fn find_user_by_email(connection: &mut PgConnection, email: &str) -> sqlx::Result<Option<User>> {
    let row = sqlx::query!("select id, password_hash from users where email = $1", email)
        .fetch_optional(connection)
        .await?;
    Ok(row.map(|row| User {
        id: UserId::from_uuid(row.id),
        password_hash: row.password_hash.map(PasswordHash::stored),
    }))
}

/// A new unverified account. `false` if the email is taken, also by a registration that committed a moment
/// ago. Java: `UserRepository.saveAndFlush` and the caught `DataIntegrityViolationException`.
pub(crate) async fn insert_user(
    connection: &mut PgConnection,
    id: UserId,
    email: &str,
    password_hash: &PasswordHash,
    now: DateTime<Utc>,
) -> sqlx::Result<bool> {
    let inserted = sqlx::query!(
        "insert into users (id, email, password_hash, verified, created_at) values ($1, $2, $3, false, $4) \
         on conflict (email) do nothing",
        id.as_uuid(),
        email,
        password_hash.as_str(),
        now,
    )
    .execute(connection)
    .await?;
    Ok(inserted.rows_affected() == 1)
}

/// Marks the account's email as confirmed. Java: `User.markVerified`.
pub(crate) async fn mark_verified(connection: &mut PgConnection, user: UserId) -> sqlx::Result<()> {
    sqlx::query!("update users set verified = true where id = $1", user.as_uuid())
        .execute(connection)
        .await?;
    Ok(())
}

/// A single-use verification token's row. Java: `new EmailVerification(userId, tokenHash, expiresAt)`.
pub(crate) async fn insert_verification(
    connection: &mut PgConnection,
    user: UserId,
    token: &TokenHash,
    expires_at: DateTime<Utc>,
) -> sqlx::Result<()> {
    sqlx::query!(
        "insert into email_verifications (id, user_id, token_hash, expires_at) values ($1, $2, $3, $4)",
        Uuid::now_v7(),
        user.as_uuid(),
        token.as_str(),
        expires_at,
    )
    .execute(connection)
    .await?;
    Ok(())
}

/// A verification as `VerifyEmail` needs it.
#[derive(Debug, Clone)]
pub(crate) struct Verification {
    pub id: Uuid,
    pub user: UserId,
    pub expires_at: DateTime<Utc>,
    pub used_at: Option<DateTime<Utc>>,
}

impl Verification {
    /// Unused and not expired. Java: `EmailVerification.isUsable`.
    pub(crate) fn is_usable(&self, now: DateTime<Utc>) -> bool {
        self.used_at.is_none() && self.expires_at > now
    }
}

/// The verification for this token hash, locked. Java: `EmailVerificationRepository.findByTokenHash`.
pub(crate) async fn find_verification_for_update(
    connection: &mut PgConnection,
    token: &TokenHash,
) -> sqlx::Result<Option<Verification>> {
    let row = sqlx::query!(
        "select id, user_id, expires_at, used_at from email_verifications where token_hash = $1 for update",
        token.as_str(),
    )
    .fetch_optional(connection)
    .await?;
    Ok(row.map(|row| Verification {
        id: row.id,
        user: UserId::from_uuid(row.user_id),
        expires_at: row.expires_at,
        used_at: row.used_at,
    }))
}

/// Burns the token. Java: `EmailVerification.markUsed`.
pub(crate) async fn mark_verification_used(
    connection: &mut PgConnection,
    id: Uuid,
    now: DateTime<Utc>,
) -> sqlx::Result<()> {
    sqlx::query!("update email_verifications set used_at = $2 where id = $1", id, now)
        .execute(connection)
        .await?;
    Ok(())
}

/// One link in a session's chain of refresh tokens. Java: `RefreshToken`.
#[derive(Debug, Clone)]
pub(crate) struct RefreshToken {
    pub id: Uuid,
    pub family: Uuid,
    pub user: UserId,
    pub expires_at: DateTime<Utc>,
    pub revoked_at: Option<DateTime<Utc>>,
    pub replaced_by: Option<Uuid>,
}

impl RefreshToken {
    /// Expired once `expires_at` is not in the future. Java: `isExpired`.
    pub(crate) fn is_expired(&self, now: DateTime<Utc>) -> bool {
        self.expires_at <= now
    }
}

/// A new refresh token row, its id a v7. Java: `new RefreshToken(familyId, userId, tokenHash, now, expiresAt)`.
pub(crate) async fn insert_refresh_token(
    connection: &mut PgConnection,
    family: Uuid,
    user: UserId,
    token: &TokenHash,
    now: DateTime<Utc>,
    expires_at: DateTime<Utc>,
) -> sqlx::Result<Uuid> {
    let id = Uuid::now_v7();
    sqlx::query!(
        "insert into refresh_tokens (id, family_id, user_id, token_hash, created_at, expires_at) \
         values ($1, $2, $3, $4, $5, $6)",
        id,
        family,
        user.as_uuid(),
        token.as_str(),
        now,
        expires_at,
    )
    .execute(connection)
    .await?;
    Ok(id)
}

/// The refresh token with this hash, locked so that two refreshes with it are serialised and the second sees it
/// as replaced. Java: `RefreshTokenRepository.findByTokenHash`.
pub(crate) async fn find_refresh_token_for_update(
    connection: &mut PgConnection,
    token: &TokenHash,
) -> sqlx::Result<Option<RefreshToken>> {
    let row = sqlx::query!(
        "select id, family_id, user_id, expires_at, revoked_at, replaced_by from refresh_tokens \
         where token_hash = $1 for update",
        token.as_str(),
    )
    .fetch_optional(connection)
    .await?;
    Ok(row.map(|row| RefreshToken {
        id: row.id,
        family: row.family_id,
        user: UserId::from_uuid(row.user_id),
        expires_at: row.expires_at,
        revoked_at: row.revoked_at,
        replaced_by: row.replaced_by,
    }))
}

/// Records that `token` was exchanged for `successor`. Java: `RefreshToken.replaceWith`.
pub(crate) async fn mark_replaced(connection: &mut PgConnection, token: Uuid, successor: Uuid) -> sqlx::Result<()> {
    sqlx::query!(
        "update refresh_tokens set replaced_by = $2 where id = $1",
        token,
        successor
    )
    .execute(connection)
    .await?;
    Ok(())
}

/// Revokes every still-valid token of the session. Java: `RefreshTokenRepository.revokeFamily`.
pub(crate) async fn revoke_family(
    connection: &mut PgConnection,
    family: Uuid,
    now: DateTime<Utc>,
) -> sqlx::Result<u64> {
    let revoked = sqlx::query!(
        "update refresh_tokens set revoked_at = $2 where family_id = $1 and revoked_at is null",
        family,
        now,
    )
    .execute(connection)
    .await?;
    Ok(revoked.rows_affected())
}
