//! Where uploaded files live: receipts under `receipts/` (and `quarantine/` once infected), imports under
//! `imports/`.
//!
//! Java counterpart: `shared/storage/ObjectStorage.java` and `S3ObjectStorage.java`. The port is a trait; the
//! adapter, [`S3ObjectStorage`], talks to any S3-compatible store (SeaweedFS locally). Tests of other crates
//! use [`testing::InMemoryStorage`]. Keys are made up by the application, never by a client.

mod s3;
#[cfg(feature = "testing")]
pub mod testing;
#[cfg(test)]
mod tests;

use std::fmt;
use std::path::Path;
use std::pin::Pin;
use std::time::Duration;

use tokio::io::AsyncRead;

pub use s3::S3ObjectStorage;

/// A future borrowed from the storage and its arguments.
pub type StorageFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, StorageError>> + Send + 'a>>;

/// The content of a stored object, read as it arrives.
pub type ObjectReader = Pin<Box<dyn AsyncRead + Send>>;

/// Port: an object store with private objects. Every method names the object by its key.
pub trait ObjectStorage: fmt::Debug + Send + Sync {
    /// Stores the file under `key` with `content_type`, replacing whatever was there.
    fn put<'a>(&'a self, key: &'a str, file: &'a Path, content_type: &'a str) -> StorageFuture<'a, ()>;

    /// The object's content. A missing key is an error.
    fn open<'a>(&'a self, key: &'a str) -> StorageFuture<'a, ObjectReader>;

    /// Removes the object; a missing key is not an error.
    fn delete<'a>(&'a self, key: &'a str) -> StorageFuture<'a, ()>;

    /// Copies, then deletes: used to quarantine an infected file.
    fn relocate<'a>(&'a self, from: &'a str, to: &'a str) -> StorageFuture<'a, ()>;

    /// A link valid for `ttl` that downloads the object as an attachment named `download_name`, of type
    /// `content_type`. The store is private; this link is the only way in.
    fn presign_get<'a>(
        &'a self,
        key: &'a str,
        download_name: &'a str,
        content_type: &'a str,
        ttl: Duration,
    ) -> StorageFuture<'a, String>;
}

/// The store could not do what was asked. The message names the operation, never a key, a file name or a
/// credential: keys are not secret, but they are no business of a log reader either.
#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    /// The object does not exist.
    #[error("object storage {operation}: no such object")]
    NotFound { operation: &'static str },
    /// The store answered with an error, or could not be reached.
    #[error("object storage {operation} failed: {reason}")]
    Failed { operation: &'static str, reason: String },
}
