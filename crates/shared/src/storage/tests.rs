//! The S3 adapter against a real S3-compatible store: SeaweedFS from the image `compose.yaml` pins.
//!
//! Java: `src/test/java/com/ledgerly/shared/storage/S3ObjectStorageTest.java`. Its observation test (every
//! call a `ledgerly.s3` timer and span tagged with the operation) arrives with the metrics in M10.3.

use std::time::Duration;

use secrecy::SecretString;
use tokio::io::AsyncReadExt;

use super::testing::TestObjectStore;
use super::{ObjectStorage, S3ObjectStorage, StorageError};
use crate::config::S3Config;

async fn staged(content: &str) -> tempfile::NamedTempFile {
    let file = tempfile::NamedTempFile::new().unwrap();
    tokio::fs::write(file.path(), content).await.unwrap();
    file
}

async fn read(storage: &S3ObjectStorage, key: &str) -> Result<String, StorageError> {
    let mut reader = storage.open(key).await?;
    let mut content = String::new();
    reader.read_to_string(&mut content).await.unwrap();
    Ok(content)
}

/// Java: `putThenOpenReturnsTheSameBytes`, `moveRelocatesTheObject`, `deleteRemovesTheObject`, against one
/// store.
#[tokio::test]
async fn put_open_move_and_delete() {
    let store = TestObjectStore::start().await.unwrap();
    let storage = store.storage();

    storage
        .put("receipts/a", staged("hello receipt").await.path(), "application/pdf")
        .await
        .unwrap();
    assert_eq!(read(&storage, "receipts/a").await.unwrap(), "hello receipt");

    storage
        .put("receipts/b", staged("infected").await.path(), "application/pdf")
        .await
        .unwrap();
    storage.relocate("receipts/b", "quarantine/b").await.unwrap();
    assert_eq!(read(&storage, "quarantine/b").await.unwrap(), "infected");
    assert!(matches!(
        read(&storage, "receipts/b").await,
        Err(StorageError::NotFound { .. })
    ));

    storage
        .put("receipts/c", staged("bye").await.path(), "application/pdf")
        .await
        .unwrap();
    storage.delete("receipts/c").await.unwrap();
    assert!(matches!(
        read(&storage, "receipts/c").await,
        Err(StorageError::NotFound { .. })
    ));
    // A missing key is not an error.
    storage.delete("receipts/never-there").await.unwrap();
}

/// Java: `aPresignedLinkDownloadsAsAnAttachmentUnderTheGivenName`.
#[tokio::test]
async fn a_presigned_link_downloads_as_an_attachment_under_the_given_name() {
    let store = TestObjectStore::start().await.unwrap();
    let storage = store.storage();
    storage
        .put("receipts/d", staged("%PDF-1.4 content").await.path(), "application/pdf")
        .await
        .unwrap();

    let url = storage
        .presign_get("receipts/d", "lunch.pdf", "application/pdf", Duration::from_secs(300))
        .await
        .unwrap();

    assert!(url.contains("X-Amz-Expires=300"), "{url}");
    assert!(url.contains("X-Amz-Signature="), "{url}");
    let response = reqwest::get(&url).await.unwrap();
    assert_eq!(response.status(), 200);
    let header = |name: &str| {
        response
            .headers()
            .get(name)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_owned()
    };
    let disposition = header("content-disposition");
    assert!(
        disposition.contains("attachment") && disposition.contains("lunch.pdf"),
        "{disposition}"
    );
    assert_eq!(header("content-type"), "application/pdf");
    assert_eq!(response.text().await.unwrap(), "%PDF-1.4 content");
}

/// `ensure_bucket` creates a missing bucket and leaves an existing one alone.
///
/// Java's `put` also creates the bucket when the store answers `NoSuchBucket`, as S3 does. SeaweedFS answers
/// `AccessDenied` instead, so that path cannot be exercised here; it is the same check as Java's.
#[tokio::test]
async fn ensure_bucket_creates_a_missing_bucket_once() {
    let store = TestObjectStore::start().await.unwrap();
    let mut config = store.config().clone();
    config.bucket = format!("fresh-{}", uuid::Uuid::new_v4().simple());
    let storage = S3ObjectStorage::new(&config);

    storage.ensure_bucket().await.unwrap();
    storage
        .put("receipts/e", staged("first").await.path(), "application/pdf")
        .await
        .unwrap();
    storage.ensure_bucket().await.unwrap();

    assert_eq!(read(&storage, "receipts/e").await.unwrap(), "first");
}

/// A store that cannot be reached is an error naming the operation, and nothing about the key.
#[tokio::test]
async fn an_unreachable_store_is_an_error_without_the_key() {
    // A port nothing listens on.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let config = S3Config {
        endpoint: format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port()),
        region: S3Config::DEFAULT_REGION.to_owned(),
        bucket: "files".to_owned(),
        access_key: SecretString::from("key"),
        secret_key: SecretString::from("secret"),
    };
    drop(listener);
    let storage = S3ObjectStorage::new(&config);

    let error = storage
        .put("receipts/secret-name", staged("x").await.path(), "application/pdf")
        .await
        .unwrap_err();

    let message = error.to_string();
    assert!(message.starts_with("object storage put failed"), "{message}");
    assert!(!message.contains("secret-name"), "{message}");
    assert!(storage.ensure_bucket().await.is_err());
}
