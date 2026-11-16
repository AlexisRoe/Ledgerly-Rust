//! The object store adapter for any S3-compatible service. Java counterpart: `shared/storage/S3ObjectStorage.java`.

use std::fmt;
use std::path::Path;
use std::time::Duration;

use aws_sdk_s3::config::{BehaviorVersion, Credentials, Region};
use aws_sdk_s3::error::{ProvideErrorMetadata, SdkError};
use aws_sdk_s3::presigning::PresigningConfig;
use aws_sdk_s3::primitives::ByteStream;
use aws_smithy_http_client::tls::{self, rustls_provider::CryptoMode};
use secrecy::ExposeSecret;
use tracing::Instrument;

use super::{ObjectReader, ObjectStorage, StorageError, StorageFuture};
use crate::config::S3Config;
use crate::observation::Observation;

/// The adapter. The bucket is private; objects are only ever reached through presigned links.
///
/// Every call runs in a `ledgerly.s3` span with the operation as `operation`, never the key: keys and file
/// names are high-cardinality and may be personal (Java's observation of the same name).
#[derive(Clone)]
pub struct S3ObjectStorage {
    client: aws_sdk_s3::Client,
    bucket: String,
}

impl fmt::Debug for S3ObjectStorage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("S3ObjectStorage")
            .field("bucket", &self.bucket)
            .finish_non_exhaustive()
    }
}

impl S3ObjectStorage {
    /// A client for `config`, with path-style addressing (`host/bucket/key`): S3-compatible stores rarely
    /// support bucket sub-domains. Credentials come from `config` only, never from the environment or an
    /// instance metadata service, so the client reaches no other host. Nothing is sent until the first call.
    #[must_use]
    pub fn new(config: &S3Config) -> Self {
        let http = aws_smithy_http_client::Builder::new()
            .tls_provider(tls::Provider::Rustls(CryptoMode::Ring))
            .build_https();
        let credentials = Credentials::new(
            config.access_key.expose_secret(),
            config.secret_key.expose_secret(),
            None,
            None,
            "ledgerly-configuration",
        );
        let s3 = aws_sdk_s3::Config::builder()
            .behavior_version(BehaviorVersion::latest())
            .region(Region::new(config.region.clone()))
            .endpoint_url(&config.endpoint)
            .credentials_provider(credentials)
            .force_path_style(true)
            .http_client(http)
            .build();
        Self {
            client: aws_sdk_s3::Client::from_conf(s3),
            bucket: config.bucket.clone(),
        }
    }

    /// Creates the bucket if it is missing. Java runs this once the application is ready, not while it
    /// starts, so that a slow or absent store does not delay the start; the app does the same by calling it
    /// from a task. [`ObjectStorage::put`] also creates a missing bucket, so an upload never depends on this.
    ///
    /// # Errors
    ///
    /// If the store cannot be reached or refuses. The caller logs it; uploads fail until the store is back.
    pub async fn ensure_bucket(&self) -> Result<(), StorageError> {
        let exists = self
            .client
            .head_bucket()
            .bucket(&self.bucket)
            .send()
            .instrument(span("ensure-bucket"))
            .await;
        match exists {
            Ok(_) => Ok(()),
            Err(error) if error.as_service_error().is_some_and(|e| e.is_not_found()) => self.create_bucket().await,
            Err(error) => Err(failed("ensure-bucket", &error)),
        }
    }

    async fn create_bucket(&self) -> Result<(), StorageError> {
        let created = self
            .client
            .create_bucket()
            .bucket(&self.bucket)
            .send()
            .instrument(span("create-bucket"))
            .await;
        match created {
            Ok(_) => Ok(()),
            // Someone else (another instance, a second test) was quicker.
            Err(error)
                if error
                    .as_service_error()
                    .is_some_and(|e| e.is_bucket_already_owned_by_you() || e.is_bucket_already_exists()) =>
            {
                Ok(())
            }
            Err(error) => Err(failed("create-bucket", &error)),
        }
    }

    /// One upload. `Ok(false)`: the bucket does not exist.
    async fn put_object(&self, key: &str, file: &Path, content_type: &str) -> Result<bool, StorageError> {
        let body = ByteStream::from_path(file).await.map_err(|_| StorageError::Failed {
            operation: "put",
            reason: "the staged file cannot be read".to_owned(),
        })?;
        let sent = self
            .client
            .put_object()
            .bucket(&self.bucket)
            .key(key)
            .content_type(content_type)
            .body(body)
            .send()
            .await;
        match sent {
            Ok(_) => Ok(true),
            Err(error) if error.code() == Some("NoSuchBucket") => Ok(false),
            Err(error) => Err(failed("put", &error)),
        }
    }
}

impl ObjectStorage for S3ObjectStorage {
    fn put<'a>(&'a self, key: &'a str, file: &'a Path, content_type: &'a str) -> StorageFuture<'a, ()> {
        Box::pin(observed("put", async move {
            if self.put_object(key, file, content_type).await? {
                return Ok(());
            }
            self.create_bucket().await?;
            if self.put_object(key, file, content_type).await? {
                Ok(())
            } else {
                Err(StorageError::Failed {
                    operation: "put",
                    reason: "NoSuchBucket".to_owned(),
                })
            }
        }))
    }

    fn open<'a>(&'a self, key: &'a str) -> StorageFuture<'a, ObjectReader> {
        Box::pin(observed("open", async move {
            let object = self
                .client
                .get_object()
                .bucket(&self.bucket)
                .key(key)
                .send()
                .await
                .map_err(|error| {
                    if error.as_service_error().is_some_and(|e| e.is_no_such_key()) {
                        StorageError::NotFound { operation: "open" }
                    } else {
                        failed("open", &error)
                    }
                })?;
            let reader: ObjectReader = Box::pin(object.body.into_async_read());
            Ok(reader)
        }))
    }

    fn delete<'a>(&'a self, key: &'a str) -> StorageFuture<'a, ()> {
        Box::pin(observed("delete", async move {
            self.client
                .delete_object()
                .bucket(&self.bucket)
                .key(key)
                .send()
                .await
                .map(|_| ())
                .map_err(|error| failed("delete", &error))
        }))
    }

    fn relocate<'a>(&'a self, from: &'a str, to: &'a str) -> StorageFuture<'a, ()> {
        Box::pin(observed("move", async move {
            self.client
                .copy_object()
                .bucket(&self.bucket)
                .copy_source(format!("{}/{from}", self.bucket))
                .key(to)
                .send()
                .await
                .map_err(|error| failed("move", &error))?;
            self.client
                .delete_object()
                .bucket(&self.bucket)
                .key(from)
                .send()
                .await
                .map(|_| ())
                .map_err(|error| failed("move", &error))
        }))
    }

    fn presign_get<'a>(
        &'a self,
        key: &'a str,
        download_name: &'a str,
        content_type: &'a str,
        ttl: Duration,
    ) -> StorageFuture<'a, String> {
        Box::pin(observed("presign", async move {
            let presigning = PresigningConfig::expires_in(ttl).map_err(|error| StorageError::Failed {
                operation: "presign",
                reason: error.to_string(),
            })?;
            let request = self
                .client
                .get_object()
                .bucket(&self.bucket)
                .key(key)
                .response_content_disposition(format!("attachment; filename=\"{download_name}\""))
                .response_content_type(content_type)
                .presigned(presigning)
                .await
                .map_err(|error| failed("presign", &error))?;
            Ok(request.uri().to_owned())
        }))
    }
}

fn span(operation: &'static str) -> tracing::Span {
    tracing::info_span!("ledgerly.s3", operation)
}

/// One call to the store as Java's `S3ObjectStorage.observe` records it: the timer and span `ledgerly.s3`,
/// tagged with the operation and never the key, and a log span of the same name.
async fn observed<T>(
    operation: &'static str,
    call: impl Future<Output = Result<T, StorageError>>,
) -> Result<T, StorageError> {
    let contextual_name = match operation {
        "put" => "s3 put",
        "open" => "s3 open",
        "delete" => "s3 delete",
        "move" => "s3 move",
        _ => "s3 presign",
    };
    let mut observation = Observation::start("ledgerly.s3", contextual_name).with_tag("operation", operation);
    let result = call.instrument(span(operation)).await;
    if let Err(error) = &result {
        // Java tags the exception's class; these are the AWS SDK's for a missing object and any other failure.
        observation.error(match error {
            StorageError::NotFound { .. } => "NoSuchKeyException",
            StorageError::Failed { .. } => "S3Exception",
        });
    }
    observation.stop();
    result
}

/// The SDK's error, reduced to what may be logged: the store's error code, or the kind of failure. The SDK's
/// own message can contain the request's URL, and with it the key.
fn failed<E: ProvideErrorMetadata, R>(operation: &'static str, error: &SdkError<E, R>) -> StorageError {
    let reason = match error {
        SdkError::ServiceError(service) => service.err().code().unwrap_or("unknown error code").to_owned(),
        SdkError::TimeoutError(_) => "timed out".to_owned(),
        SdkError::DispatchFailure(_) => "could not reach the store".to_owned(),
        SdkError::ResponseError(_) => "unreadable response".to_owned(),
        SdkError::ConstructionFailure(_) => "request could not be built".to_owned(),
        _ => "unknown failure".to_owned(),
    };
    StorageError::Failed { operation, reason }
}
