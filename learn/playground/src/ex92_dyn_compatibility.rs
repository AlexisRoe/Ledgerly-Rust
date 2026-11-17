//! Lesson 92: `dyn` compatibility and async in traits. Run `cargo test ex92`.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;

/// A key-value store, usable as `&dyn Store` because every method that would break that is marked
/// `where Self: Sized` ("not available on trait objects").
pub trait Store {
    fn get(&self, key: &str) -> Option<String>;
    fn put(&mut self, key: &str, value: String);

    /// A generic method: there is no single function to put in a vtable, so it's excluded from `dyn Store`.
    /// Try deleting `where Self: Sized` here and read error E0038.
    fn put_all<I: IntoIterator<Item = (String, String)>>(&mut self, items: I)
    where
        Self: Sized,
    {
        for (key, value) in items {
            self.put(&key, value);
        }
    }
}

#[derive(Default)]
pub struct MemoryStore(HashMap<String, String>);

impl Store for MemoryStore {
    /// 1. Look the key up and clone the value out.
    fn get(&self, key: &str) -> Option<String> {
        todo!()
    }

    fn put(&mut self, key: &str, value: String) {
        todo!()
    }
}

/// 2. Works with any store chosen at run time. Copy `key` from one store to the other; return whether it existed.
pub fn copy_key(from: &dyn Store, to: &mut dyn Store, key: &str) -> bool {
    todo!()
}

/// Ledgerly's trick for async methods on `dyn` traits: return a boxed, pinned future.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

pub trait NameLookup: Send + Sync {
    fn name_of(&self, id: u32) -> BoxFuture<'_, Option<String>>;
}

pub struct FixedNames(pub HashMap<u32, String>);

impl NameLookup for FixedNames {
    /// 3. Wrap an `async move { … }` block in `Box::pin(…)`. Inside, look the id up and clone the name.
    fn name_of(&self, id: u32) -> BoxFuture<'_, Option<String>> {
        todo!()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    #[test]
    fn stores_are_interchangeable() {
        let mut from = MemoryStore::default();
        from.put_all([("team".to_owned(), "ops".to_owned())]);
        let mut to = MemoryStore::default();
        assert!(copy_key(&from, &mut to, "team"));
        assert!(!copy_key(&from, &mut to, "missing"));
        assert_eq!(to.get("team").as_deref(), Some("ops"));
    }

    #[tokio::test]
    async fn async_methods_through_dyn() {
        let lookup: Arc<dyn NameLookup> = Arc::new(FixedNames(HashMap::from([(7, "Sam".to_owned())])));
        assert_eq!(lookup.name_of(7).await.as_deref(), Some("Sam"));
        assert_eq!(lookup.name_of(8).await, None);
    }
}
