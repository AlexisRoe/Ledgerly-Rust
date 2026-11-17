//! Where links are kept. The rest of the service depends on the [`LinkStore`] port, never on an adapter, so a
//! Postgres adapter could replace [`MemoryStore`] without touching the handlers.

use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::fmt;
use std::sync::{PoisonError, RwLock};

use crate::domain::{Link, Slug};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum StoreError {
    #[error("the slug {0} is taken")]
    Taken(Slug),
}

/// Port: stores links. `Send + Sync` so that one store can be shared by every request as `Arc<dyn LinkStore>`.
///
/// The methods are synchronous because the only adapter is in memory. A database adapter would make them return
/// `BoxFuture`s, like Ledgerly's ports (lesson 92).
pub trait LinkStore: fmt::Debug + Send + Sync {
    /// # Errors
    ///
    /// [`StoreError::Taken`] if a link with the same slug exists; nothing is changed then.
    fn insert(&self, link: Link) -> Result<(), StoreError>;

    fn get(&self, slug: &Slug) -> Option<Link>;

    /// Adds `count` visits to the link, if it exists.
    fn add_visits(&self, slug: &Slug, count: u64);
}

/// The in-memory adapter. Reads (every redirect) far outnumber writes, hence an `RwLock`.
#[derive(Debug, Default)]
pub struct MemoryStore {
    links: RwLock<HashMap<Slug, Link>>,
}

impl LinkStore for MemoryStore {
    fn insert(&self, link: Link) -> Result<(), StoreError> {
        // A panic while holding the lock can't leave a half-written map entry behind, so poisoning is ignored.
        let mut links = self.links.write().unwrap_or_else(PoisonError::into_inner);
        match links.entry(link.slug.clone()) {
            Entry::Occupied(_) => Err(StoreError::Taken(link.slug)),
            Entry::Vacant(slot) => {
                slot.insert(link);
                Ok(())
            }
        }
    }

    fn get(&self, slug: &Slug) -> Option<Link> {
        self.links
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .get(slug)
            .cloned()
    }

    fn add_visits(&self, slug: &Slug, count: u64) {
        if let Some(link) = self.links.write().unwrap_or_else(PoisonError::into_inner).get_mut(slug) {
            link.visits = link.visits.saturating_add(count);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::domain::Target;

    fn link(slug: &str) -> Link {
        Link::new(
            Slug::parse(slug).unwrap(),
            Target::parse("https://example.org").unwrap(),
        )
    }

    #[test]
    fn stores_and_finds_links() {
        let store = MemoryStore::default();
        store.insert(link("abc")).unwrap();
        assert_eq!(store.get(&Slug::parse("abc").unwrap()), Some(link("abc")));
        assert_eq!(store.get(&Slug::parse("xyz").unwrap()), None);
    }

    #[test]
    fn a_slug_is_taken_once() {
        let store = MemoryStore::default();
        store.insert(link("abc")).unwrap();
        assert_eq!(
            store.insert(link("abc")),
            Err(StoreError::Taken(Slug::parse("abc").unwrap()))
        );
    }

    #[test]
    fn counts_visits() {
        let store = MemoryStore::default();
        store.insert(link("abc")).unwrap();
        let slug = Slug::parse("abc").unwrap();
        store.add_visits(&slug, 2);
        store.add_visits(&slug, 3);
        store.add_visits(&Slug::parse("nope").unwrap(), 1);
        assert_eq!(store.get(&slug).map(|l| l.visits), Some(5));
    }

    #[test]
    fn is_shareable_between_threads() {
        let store: Arc<dyn LinkStore> = Arc::new(MemoryStore::default());
        std::thread::scope(|scope| {
            for n in 0..4 {
                let store = &store;
                scope.spawn(move || store.insert(link(&format!("link-{n}"))).unwrap());
            }
        });
        assert!(store.get(&Slug::parse("link-3").unwrap()).is_some());
    }
}
