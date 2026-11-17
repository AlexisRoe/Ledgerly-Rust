//! Counting visits without slowing down redirects. A redirect only sends the slug into a channel; this task
//! collects them and writes the counts to the store in batches.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc;

use crate::domain::Slug;
use crate::store::LinkStore;

/// Runs until every sender is dropped, then writes what's left and returns. The server's state holds the
/// senders, so stopping the server is what stops this task: no separate shutdown signal needed.
pub async fn run(store: Arc<dyn LinkStore>, mut visits: mpsc::Receiver<Slug>, flush_every: Duration) {
    let mut pending: HashMap<Slug, u64> = HashMap::new();
    let mut tick = tokio::time::interval(flush_every);
    loop {
        tokio::select! {
            // `recv` is cancel-safe: if the tick wins, no visit is lost.
            received = visits.recv() => match received {
                Some(slug) => *pending.entry(slug).or_insert(0) += 1,
                None => break,
            },
            _ = tick.tick() => flush(store.as_ref(), &mut pending),
        }
    }
    flush(store.as_ref(), &mut pending);
}

fn flush(store: &dyn LinkStore, pending: &mut HashMap<Slug, u64>) {
    for (slug, count) in pending.drain() {
        store.add_visits(&slug, count);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{Link, Target};
    use crate::store::MemoryStore;

    #[tokio::test]
    async fn writes_every_visit_before_it_ends() {
        let store: Arc<dyn LinkStore> = Arc::new(MemoryStore::default());
        let slug = Slug::parse("abc").unwrap();
        store
            .insert(Link::new(slug.clone(), Target::parse("https://example.org").unwrap()))
            .unwrap();
        let (sender, receiver) = mpsc::channel(16);
        let worker = tokio::spawn(run(Arc::clone(&store), receiver, Duration::from_secs(3600)));
        for _ in 0..3 {
            sender.send(slug.clone()).await.unwrap();
        }
        drop(sender);
        worker.await.unwrap();
        assert_eq!(store.get(&slug).map(|l| l.visits), Some(3));
    }

    #[tokio::test]
    async fn flushes_on_the_timer_while_running() {
        let store: Arc<dyn LinkStore> = Arc::new(MemoryStore::default());
        let slug = Slug::parse("abc").unwrap();
        store
            .insert(Link::new(slug.clone(), Target::parse("https://example.org").unwrap()))
            .unwrap();
        let (sender, receiver) = mpsc::channel(16);
        let worker = tokio::spawn(run(Arc::clone(&store), receiver, Duration::from_millis(10)));
        sender.send(slug.clone()).await.unwrap();
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(store.get(&slug).map(|l| l.visits), Some(1));
        drop(sender);
        worker.await.unwrap();
    }
}
