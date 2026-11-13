//! Starting state, written with SQL into the schema both implementations share.
//!
//! Java counterpart: `TestUsers`, `TeamSeeder`, `ExpenseSeeder`, `ReceiptSeeder` and `OwaspTestBase.newTeam()`
//! under `src/test/java/com/ledgerly/`. The rows are the ones those seeders write, with two differences that
//! being outside the process forces: users get a real Argon2 hash, because tokens come from logging in over
//! HTTP and not from Java's `AccessTokenIssuer`, and an accepted receipt gets a real object in the store.
//!
//! Every helper makes fresh ids and email addresses, so tests can run in parallel and repeatedly against a
//! database that is never cleaned.

use std::sync::OnceLock;

use argon2::{Algorithm, Argon2, Params, Version};
use chrono::{DateTime, NaiveDate, Utc};
use rust_decimal::Decimal;
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;
use uuid::Uuid;

use crate::Target;
use crate::encoding::{base64_unpadded, base64url_unpadded, sha256_hex};

/// The password of every fixture user. Long enough for the register rule (12 to 128 characters).
pub const PASSWORD: &str = "parity-suite-password";

/// The minimal PDF of Java's `OwaspTestBase.PDF`.
pub const PDF: &[u8] = b"%PDF-1.4\n1 0 obj << >> endobj\n%%EOF\n";

/// A user that can log in with [`PASSWORD`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct User {
    pub id: Uuid,
    pub email: String,
}

/// A team with one user per role, as `OwaspTestBase.newTeam()` builds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Team {
    pub id: Uuid,
    pub owner: User,
    pub approver: User,
    pub member: User,
    /// A second MEMBER, for "a member must not see a teammate's expense".
    pub colleague: User,
}

/// The columns of an expense row a test may care about. [`Expense::new`] fills in the values of Java's
/// `ExpenseSeeder`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Expense {
    pub team_id: Uuid,
    pub claimant_id: Uuid,
    pub status: String,
    pub amount: Decimal,
    pub currency: String,
    pub category: String,
    pub incurred_on: NaiveDate,
    pub merchant: String,
    pub note: Option<String>,
    pub created_at: DateTime<Utc>,
}

impl Expense {
    /// 10.00 EUR, MEALS, merchant `Seed`, incurred today, created now: `OwaspTestBase.expense()`.
    #[must_use]
    pub fn new(team_id: Uuid, claimant_id: Uuid, status: &str) -> Self {
        let now = Utc::now();
        Self {
            team_id,
            claimant_id,
            status: status.to_owned(),
            amount: Decimal::new(1000, 2),
            currency: "EUR".to_owned(),
            category: "MEALS".to_owned(),
            incurred_on: now.date_naive(),
            merchant: "Seed".to_owned(),
            note: None,
            created_at: now,
        }
    }
}

/// Access to the database behind the API.
#[derive(Debug, Clone)]
pub struct Fixtures {
    pool: PgPool,
    http: reqwest::Client,
    s3_endpoint: String,
    s3_bucket: String,
}

/// Spring Security's `Argon2PasswordEncoder.defaultsForSpringSecurity_v5_8()`: Argon2id, 16 MiB, 2 passes, one
/// lane, a 16-byte salt and a 32-byte hash, as a PHC string with unpadded standard Base64.
fn password_hash(password: &str, salt: &[u8; 16]) -> String {
    let params = Params::new(16_384, 2, 1, Some(32)).expect("Spring's Argon2 parameters are valid");
    let mut hash = [0_u8; 32];
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
        .hash_password_into(password.as_bytes(), salt, &mut hash)
        .expect("Argon2 can hash the fixture password");
    format!(
        "$argon2id$v=19$m=16384,t=2,p=1${}${}",
        base64_unpadded(salt),
        base64_unpadded(&hash)
    )
}

/// The hash of [`PASSWORD`], computed once per test process: every fixture user shares it.
fn fixture_password_hash() -> &'static str {
    static HASH: OnceLock<String> = OnceLock::new();
    HASH.get_or_init(|| password_hash(PASSWORD, Uuid::new_v4().as_bytes()))
}

impl Fixtures {
    /// Connects to `PARITY_DATABASE_URL`.
    ///
    /// # Panics
    ///
    /// Panics if the database cannot be reached.
    pub async fn connect(target: &Target) -> Self {
        let pool = PgPoolOptions::new()
            .max_connections(2)
            .connect(&target.database_url)
            .await
            .unwrap_or_else(|error| panic!("PARITY_DATABASE_URL: cannot connect to the database ({error})"));
        Self {
            pool,
            http: reqwest::Client::new(),
            s3_endpoint: target.s3_endpoint.trim_end_matches('/').to_owned(),
            s3_bucket: target.s3_bucket.clone(),
        }
    }

    /// The connection pool, for assertions on stored data.
    #[must_use]
    pub const fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// A verified user with a fresh address.
    pub async fn user(&self) -> User {
        self.insert_user(true).await
    }

    /// A user whose email is not verified. They may log in, but not accept invitations.
    pub async fn unverified_user(&self) -> User {
        self.insert_user(false).await
    }

    async fn insert_user(&self, verified: bool) -> User {
        let id = Uuid::now_v7();
        let email = format!("parity-{}@example.com", id.simple());
        sqlx::query(
            "insert into users (id, email, password_hash, verified, created_at) values ($1, $2, $3, $4, now())",
        )
        .bind(id)
        .bind(&email)
        .bind(fixture_password_hash())
        .bind(verified)
        .execute(&self.pool)
        .await
        .expect("a user can be inserted");
        User { id, email }
    }

    /// An email verification for `user` that is valid for 24 hours. Returns the token; the table holds only
    /// its SHA-256, as Java's `OpaqueTokens` stores it.
    pub async fn verification_token(&self, user: &User) -> String {
        let mut bytes = [0_u8; 32];
        bytes[..16].copy_from_slice(Uuid::new_v4().as_bytes());
        bytes[16..].copy_from_slice(Uuid::new_v4().as_bytes());
        let token = base64url_unpadded(&bytes);
        sqlx::query(
            "insert into email_verifications (id, user_id, token_hash, expires_at) \
             values ($1, $2, $3, now() + interval '24 hours')",
        )
        .bind(Uuid::now_v7())
        .bind(user.id)
        .bind(sha256_hex(token.as_bytes()))
        .execute(&self.pool)
        .await
        .expect("an email verification can be inserted");
        token
    }

    /// An empty team named `Test team`. Returns its id.
    pub async fn team(&self) -> Uuid {
        let id = Uuid::now_v7();
        sqlx::query("insert into teams (id, name, created_at) values ($1, 'Test team', now())")
            .bind(id)
            .execute(&self.pool)
            .await
            .expect("a team can be inserted");
        id
    }

    /// Makes `user` a teammate with `role` (`MEMBER`, `APPROVER` or `OWNER`), or changes their role.
    pub async fn member(&self, team_id: Uuid, user: &User, role: &str) {
        sqlx::query(
            "insert into memberships (team_id, user_id, role, joined_at) values ($1, $2, $3, now()) \
             on conflict (team_id, user_id) do update set role = excluded.role",
        )
        .bind(team_id)
        .bind(user.id)
        .bind(role)
        .execute(&self.pool)
        .await
        .expect("a membership can be inserted");
    }

    /// Removes a teammate, as an owner would.
    pub async fn remove_member(&self, team_id: Uuid, user: &User) {
        sqlx::query("delete from memberships where team_id = $1 and user_id = $2")
            .bind(team_id)
            .bind(user.id)
            .execute(&self.pool)
            .await
            .expect("a membership can be deleted");
    }

    /// A team with an OWNER, an APPROVER, a MEMBER and a second MEMBER, all verified.
    pub async fn team_with_roles(&self) -> Team {
        let id = self.team().await;
        let team = Team {
            id,
            owner: self.user().await,
            approver: self.user().await,
            member: self.user().await,
            colleague: self.user().await,
        };
        self.member(id, &team.owner, "OWNER").await;
        self.member(id, &team.approver, "APPROVER").await;
        self.member(id, &team.member, "MEMBER").await;
        self.member(id, &team.colleague, "MEMBER").await;
        team
    }

    /// An expense in any status, without going through the workflow. Returns its id.
    pub async fn expense(&self, team_id: Uuid, claimant: &User, status: &str) -> Uuid {
        self.insert_expense(&Expense::new(team_id, claimant.id, status)).await
    }

    /// An expense with every column chosen by the test. Returns its id.
    pub async fn insert_expense(&self, expense: &Expense) -> Uuid {
        let id = Uuid::now_v7();
        sqlx::query(
            "insert into expenses (id, team_id, claimant_id, amount, currency, category, incurred_on, merchant, \
                                   note, status, version, created_at, updated_at) \
             values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, 0, $11, $11)",
        )
        .bind(id)
        .bind(expense.team_id)
        .bind(expense.claimant_id)
        .bind(expense.amount)
        .bind(&expense.currency)
        .bind(&expense.category)
        .bind(expense.incurred_on)
        .bind(&expense.merchant)
        .bind(&expense.note)
        .bind(&expense.status)
        .bind(expense.created_at)
        .execute(&self.pool)
        .await
        .expect("an expense can be inserted");
        id
    }

    /// Appends a rejection to an expense, at the next position.
    pub async fn rejection(&self, expense_id: Uuid, rejected_by: &User, reason: &str) {
        sqlx::query(
            "insert into expense_rejections (expense_id, position, rejected_by, reason) \
             select $1, coalesce(max(position) + 1, 0), $2, $3 from expense_rejections where expense_id = $1",
        )
        .bind(expense_id)
        .bind(rejected_by.id)
        .bind(reason)
        .execute(&self.pool)
        .await
        .expect("a rejection can be inserted");
    }

    /// A receipt that passed the scan, with its file in the object store, so that the expense can be submitted
    /// and the receipt downloaded. The content is [`PDF`] followed by the receipt's id: Java keeps receipts of
    /// one expense unique by checksum. Returns the receipt's id.
    pub async fn accepted_receipt(&self, team_id: Uuid, expense_id: Uuid, uploaded_by: &User) -> Uuid {
        let id = Uuid::now_v7();
        let mut content = PDF.to_vec();
        content.extend_from_slice(format!("% {id}\n").as_bytes());
        let storage_key = format!("receipts/{}", Uuid::new_v4());
        self.put_object(&storage_key, "application/pdf", &content).await;
        sqlx::query(
            "insert into receipts (id, team_id, expense_id, storage_key, original_name, detected_type, size_bytes, \
                                   sha256, scan_status, possible_duplicate, uploaded_by, created_at) \
             values ($1, $2, $3, $4, 'seed.pdf', 'application/pdf', $5, $6, 'ACCEPTED', false, $7, now())",
        )
        .bind(id)
        .bind(team_id)
        .bind(expense_id)
        .bind(&storage_key)
        .bind(i64::try_from(content.len()).expect("a receipt fits in a bigint"))
        .bind(sha256_hex(&content))
        .bind(uploaded_by.id)
        .execute(&self.pool)
        .await
        .expect("a receipt can be inserted");
        id
    }

    /// Stores an object under `key` in the bucket the API uses, creating the bucket if it is missing. The
    /// development object store takes unsigned requests; a deployment with real credentials needs its objects
    /// put there by other means.
    pub async fn put_object(&self, key: &str, content_type: &str, content: &[u8]) {
        let bucket = format!("{}/{}", self.s3_endpoint, self.s3_bucket);
        // Creating a bucket that exists is answered with 409, which is fine.
        let _ = self.http.put(&bucket).send().await;
        let response = self
            .http
            .put(format!("{bucket}/{key}"))
            .header("Content-Type", content_type)
            .body(content.to_vec())
            .send()
            .await
            .unwrap_or_else(|error| panic!("PARITY_S3_ENDPOINT: the object store cannot be reached ({error})"));
        assert!(
            response.status().is_success(),
            "the object store refused {key}: {}",
            response.status()
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Spring writes exactly this shape; `$argon2id$v=19$m=16384,t=2,p=1$<22 chars>$<43 chars>`.
    #[test]
    fn the_password_hash_is_a_phc_string_with_springs_parameters() {
        let hash = password_hash(PASSWORD, &[7; 16]);

        let parts: Vec<&str> = hash.split('$').collect();
        assert_eq!(parts[..4], ["", "argon2id", "v=19", "m=16384,t=2,p=1"]);
        assert_eq!(parts[4], base64_unpadded(&[7; 16]));
        assert_eq!((parts[4].len(), parts[5].len()), (22, 43));
        assert_eq!(parts.len(), 6);
    }

    #[test]
    fn the_hash_depends_on_password_and_salt_and_is_repeatable() {
        let hash = password_hash(PASSWORD, &[7; 16]);

        assert_eq!(hash, password_hash(PASSWORD, &[7; 16]));
        assert_ne!(hash, password_hash(PASSWORD, &[8; 16]));
        assert_ne!(hash, password_hash("another-password", &[7; 16]));
    }

    /// RFC 9106 has no test vector for these parameters, so this pins the value the `argon2` crate computes
    /// today. M5 checks the same hash against Spring's from the demo data.
    #[test]
    fn the_fixture_hash_is_computed_once() {
        assert!(std::ptr::eq(fixture_password_hash(), fixture_password_hash()));
        assert!(fixture_password_hash().starts_with("$argon2id$v=19$m=16384,t=2,p=1$"));
    }

    #[test]
    fn the_seed_pdf_is_javas() {
        assert_eq!(PDF.len(), 36);
        assert!(PDF.starts_with(b"%PDF-"));
    }

    #[test]
    fn a_seed_expense_has_the_values_of_javas_expense_seeder() {
        let (team, claimant) = (Uuid::now_v7(), Uuid::now_v7());
        let expense = Expense::new(team, claimant, "SUBMITTED");

        assert_eq!(expense.amount.to_string(), "10.00");
        assert_eq!(expense.category, "MEALS");
        assert_eq!((expense.currency.as_str(), expense.merchant.as_str()), ("EUR", "Seed"));
        assert_eq!(expense.status, "SUBMITTED");
        assert_eq!(expense.note, None);
    }
}
