//! Lesson 113: constructors, `Default` and builders. Run `cargo test ex113`.

use std::time::Duration;

/// Settings with sensible defaults, like Ledgerly's `Limits`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Limits {
    pub page_size: u32,
    pub batch_size: u32,
    pub upload_bytes: u64,
}

/// 1. Defaults: page 100, batch 100, upload 10 MiB (10 * 1024 * 1024). A hand-written impl, because the
///    derived one would give zeros.
impl Default for Limits {
    fn default() -> Self {
        todo!()
    }
}

/// 2. Struct update syntax: the defaults, except a page size of 20. `Limits { page_size: 20, ..Default::default() }`
pub fn small_pages() -> Limits {
    todo!()
}

/// An HTTP client config with one required field and several optional ones: a job for a builder.
#[derive(Debug, PartialEq, Eq)]
pub struct Client {
    pub base_url: String,
    pub timeout: Duration,
    pub retries: u8,
}

#[derive(Debug)]
#[must_use = "a builder does nothing until .build() is called"]
pub struct ClientBuilder {
    base_url: String,
    timeout: Duration,
    retries: u8,
}

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum BuildError {
    #[error("base URL must start with https://")]
    NotHttps,
    #[error("at most 5 retries")]
    TooManyRetries,
}

impl Client {
    /// The required part goes into the entry point; everything else has a default (30 s, 0 retries).
    pub fn builder(base_url: impl Into<String>) -> ClientBuilder {
        ClientBuilder { base_url: base_url.into(), timeout: Duration::from_secs(30), retries: 0 }
    }
}

impl ClientBuilder {
    /// 3. Setters take `self` by value and return it, so calls chain: `Client::builder(u).timeout(t).retries(2)`.
    pub fn timeout(mut self, timeout: Duration) -> Self {
        todo!()
    }

    pub fn retries(mut self, retries: u8) -> Self {
        todo!()
    }

    /// 4. Validate everything once, at the end: https only, at most 5 retries.
    pub fn build(self) -> Result<Client, BuildError> {
        todo!()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_and_updates() {
        assert_eq!(Limits::default().upload_bytes, 10_485_760);
        assert_eq!(small_pages(), Limits { page_size: 20, ..Limits::default() });
        assert_eq!(small_pages().batch_size, 100);
    }

    #[test]
    fn builds_with_defaults_and_overrides() {
        let client = Client::builder("https://api.example.org").retries(2).build();
        assert_eq!(
            client,
            Ok(Client { base_url: "https://api.example.org".to_owned(), timeout: Duration::from_secs(30), retries: 2 })
        );
        let fast = Client::builder("https://x").timeout(Duration::from_secs(1)).build();
        assert_eq!(fast.map(|c| c.timeout), Ok(Duration::from_secs(1)));
    }

    #[test]
    fn validates_once_at_the_end() {
        assert_eq!(Client::builder("http://x").build(), Err(BuildError::NotHttps));
        assert_eq!(Client::builder("https://x").retries(9).build(), Err(BuildError::TooManyRetries));
    }
}
