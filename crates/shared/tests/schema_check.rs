//! The startup schema check against real databases built by the pinned Flyway image (plan M2.1, ADR 0006).

use ledgerly_shared::database::{Difference, SchemaError, TestDatabase, verify};

#[tokio::test]
async fn a_freshly_migrated_database_passes() {
    let database = TestDatabase::new().await.unwrap();
    verify(database.pool()).await.unwrap();
}

#[tokio::test]
async fn a_database_without_v10_is_refused_and_v10_is_named() {
    let database = TestDatabase::new().await.unwrap();
    sqlx::query("delete from flyway_schema_history where version = '10'")
        .execute(database.pool())
        .await
        .unwrap();

    let Err(SchemaError::Mismatch(mismatch)) = verify(database.pool()).await else {
        panic!("a database without V10 was accepted");
    };
    assert_eq!(
        mismatch.0,
        [Difference::NotApplied {
            version: "10",
            description: "external identity"
        }]
    );
    assert!(
        mismatch
            .to_string()
            .contains("V10 (external identity) has not been applied")
    );
}

#[tokio::test]
async fn a_tampered_checksum_is_refused_and_its_version_named() {
    let database = TestDatabase::new().await.unwrap();
    sqlx::query("update flyway_schema_history set checksum = checksum + 1 where version = '4'")
        .execute(database.pool())
        .await
        .unwrap();

    let Err(SchemaError::Mismatch(mismatch)) = verify(database.pool()).await else {
        panic!("a database with a changed V4 was accepted");
    };
    assert!(
        matches!(mismatch.0.as_slice(), [Difference::ChecksumChanged { version, .. }] if version == "4"),
        "{mismatch:?}"
    );
}

#[tokio::test]
async fn a_migration_this_build_does_not_know_is_refused() {
    let database = TestDatabase::new().await.unwrap();
    sqlx::query(
        "insert into flyway_schema_history \
           (installed_rank, version, description, type, script, checksum, installed_by, execution_time, success) \
         values (11, '11', 'from the future', 'SQL', 'V11__from_the_future.sql', 1, 'ledgerly', 1, true)",
    )
    .execute(database.pool())
    .await
    .unwrap();

    let Err(SchemaError::Mismatch(mismatch)) = verify(database.pool()).await else {
        panic!("a database with an unknown V11 was accepted");
    };
    assert_eq!(
        mismatch.0,
        [Difference::Unknown {
            version: Some("11".to_owned()),
            script: "V11__from_the_future.sql".to_owned()
        }]
    );
}

#[tokio::test]
async fn a_database_that_was_never_migrated_is_refused() {
    let database = TestDatabase::new().await.unwrap();
    sqlx::query("alter table flyway_schema_history rename to something_else")
        .execute(database.pool())
        .await
        .unwrap();

    assert!(matches!(verify(database.pool()).await, Err(SchemaError::NeverMigrated)));
}
