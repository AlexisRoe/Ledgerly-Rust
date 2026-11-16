//! Object stores for tests: a real SeaweedFS in Docker for the adapter's own tests, and an in-memory store for
//! the use cases of other crates (Java's `FileSystemObjectStorage`).
//!
//! Only compiled with the `testing` feature.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use secrecy::SecretString;

use super::{ObjectReader, ObjectStorage, S3ObjectStorage, StorageError, StorageFuture};
use crate::config::S3Config;
use crate::database::testing::{LABEL, docker, image_of};

/// A SeaweedFS of its own, from the image `compose.yaml` pins, started like [`crate::database::TestDatabase`]
/// starts Postgres: through the Docker command line, published on 127.0.0.1 only, labelled so that
/// `just test-db-down` finds it. It is removed when this value is dropped.
#[derive(Debug)]
pub struct TestObjectStore {
    container: String,
    config: S3Config,
}

impl TestObjectStore {
    /// Starts the container and waits until it accepts bucket calls, which takes a few seconds after it
    /// starts listening.
    ///
    /// # Errors
    ///
    /// If Docker cannot start it, or it does not come up within a minute.
    pub async fn start() -> Result<Self, String> {
        let image = image_of("seaweedfs").map_err(|error| error.to_string())?;
        let container = format!("ledgerly-test-seaweedfs-{}", uuid::Uuid::new_v4().simple());
        docker(&[
            "run",
            "--detach",
            "--rm",
            "--name",
            &container,
            "--publish",
            "127.0.0.1::8333",
            "--label",
            LABEL,
            image,
            "server",
            "-s3",
            "-s3.port=8333",
            "-dir=/data",
        ])
        .map_err(|error| error.to_string())?;
        // From here on, dropping `store` removes the container, also when it never comes up.
        let mut store = Self {
            container,
            config: S3Config {
                endpoint: String::new(),
                region: S3Config::DEFAULT_REGION.to_owned(),
                bucket: format!("test-{}", uuid::Uuid::new_v4().simple()),
                access_key: SecretString::from("test"),
                secret_key: SecretString::from("test-secret"),
            },
        };
        let published = docker(&["port", &store.container, "8333/tcp"]).map_err(|error| error.to_string())?;
        let port = published
            .lines()
            .find_map(|line| line.strip_prefix("127.0.0.1:"))
            .map(str::trim)
            .ok_or_else(|| format!("no port published for SeaweedFS: {published}"))?;
        store.config.endpoint = format!("http://127.0.0.1:{port}");

        let storage = store.storage();
        let deadline = tokio::time::Instant::now() + Duration::from_secs(60);
        loop {
            match storage.ensure_bucket().await {
                Ok(()) => return Ok(store),
                Err(error) if tokio::time::Instant::now() > deadline => {
                    return Err(format!("SeaweedFS never came up: {error}"));
                }
                Err(_) => tokio::time::sleep(Duration::from_millis(500)).await,
            }
        }
    }

    /// The adapter, pointed at this store and its bucket.
    #[must_use]
    pub fn storage(&self) -> S3ObjectStorage {
        S3ObjectStorage::new(&self.config)
    }

    /// The settings of this store.
    #[must_use]
    pub const fn config(&self) -> &S3Config {
        &self.config
    }
}

impl Drop for TestObjectStore {
    fn drop(&mut self) {
        // Nothing to do if it fails: the container stops with Docker, and `just test-db-down` removes it.
        let _ = docker(&["rm", "--force", "--volumes", &self.container]);
    }
}

/// One object held by [`InMemoryStorage`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredObject {
    pub content: Vec<u8>,
    pub content_type: String,
}

/// A presigned link [`InMemoryStorage`] handed out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Presigned {
    pub key: String,
    pub download_name: String,
    pub content_type: String,
    pub ttl: Duration,
}

#[derive(Debug, Default)]
struct Memory {
    objects: HashMap<String, StoredObject>,
    presigned: Vec<Presigned>,
    unavailable: bool,
}

/// The objects live in a map; clones share it. Presigned links are recorded and look like
/// `memory://<key>`. With [`InMemoryStorage::set_unavailable`] every call fails, as a store that is down.
#[derive(Debug, Clone, Default)]
pub struct InMemoryStorage(Arc<Mutex<Memory>>);

impl InMemoryStorage {
    fn memory(&self) -> std::sync::MutexGuard<'_, Memory> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The object under `key`, if there is one.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<StoredObject> {
        self.memory().objects.get(key).cloned()
    }

    /// Every key, sorted.
    #[must_use]
    pub fn keys(&self) -> Vec<String> {
        let mut keys: Vec<String> = self.memory().objects.keys().cloned().collect();
        keys.sort();
        keys
    }

    /// Every link handed out, oldest first.
    #[must_use]
    pub fn presigned(&self) -> Vec<Presigned> {
        self.memory().presigned.clone()
    }

    /// Stores `content` directly, as a test fixture.
    pub fn insert(&self, key: &str, content: &[u8], content_type: &str) {
        self.memory().objects.insert(
            key.to_owned(),
            StoredObject {
                content: content.to_vec(),
                content_type: content_type.to_owned(),
            },
        );
    }

    /// While `true`, every call fails.
    pub fn set_unavailable(&self, unavailable: bool) {
        self.memory().unavailable = unavailable;
    }

    fn available(&self, operation: &'static str) -> Result<(), StorageError> {
        if self.memory().unavailable {
            Err(StorageError::Failed {
                operation,
                reason: "unavailable".to_owned(),
            })
        } else {
            Ok(())
        }
    }
}

impl ObjectStorage for InMemoryStorage {
    fn put<'a>(&'a self, key: &'a str, file: &'a Path, content_type: &'a str) -> StorageFuture<'a, ()> {
        Box::pin(async move {
            self.available("put")?;
            let content = tokio::fs::read(file).await.map_err(|_| StorageError::Failed {
                operation: "put",
                reason: "the staged file cannot be read".to_owned(),
            })?;
            self.insert(key, &content, content_type);
            Ok(())
        })
    }

    fn open<'a>(&'a self, key: &'a str) -> StorageFuture<'a, ObjectReader> {
        Box::pin(async move {
            self.available("open")?;
            let object = self.get(key).ok_or(StorageError::NotFound { operation: "open" })?;
            let reader: ObjectReader = Box::pin(std::io::Cursor::new(object.content));
            Ok(reader)
        })
    }

    fn delete<'a>(&'a self, key: &'a str) -> StorageFuture<'a, ()> {
        Box::pin(async move {
            self.available("delete")?;
            self.memory().objects.remove(key);
            Ok(())
        })
    }

    fn relocate<'a>(&'a self, from: &'a str, to: &'a str) -> StorageFuture<'a, ()> {
        Box::pin(async move {
            self.available("move")?;
            let mut memory = self.memory();
            let object = memory
                .objects
                .remove(from)
                .ok_or(StorageError::NotFound { operation: "move" })?;
            memory.objects.insert(to.to_owned(), object);
            Ok(())
        })
    }

    fn presign_get<'a>(
        &'a self,
        key: &'a str,
        download_name: &'a str,
        content_type: &'a str,
        ttl: Duration,
    ) -> StorageFuture<'a, String> {
        Box::pin(async move {
            self.available("presign")?;
            self.memory().presigned.push(Presigned {
                key: key.to_owned(),
                download_name: download_name.to_owned(),
                content_type: content_type.to_owned(),
                ttl,
            });
            Ok(format!("memory://{key}"))
        })
    }
}
