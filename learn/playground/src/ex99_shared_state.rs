//! Lesson 99: shared mutable state. Run `cargo test ex99`.

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError, RwLock};
use std::thread;

/// 1. Count with an atomic: spawn `threads` threads that each add 1 to the counter `per_thread` times
///    (`fetch_add(1, Ordering::Relaxed)`; a plain counter needs no stronger ordering). Return the final value.
///    Share the counter with `Arc` and `thread::spawn`, joining every handle.
pub fn atomic_count(threads: usize, per_thread: usize) -> usize {
    let counter = Arc::new(AtomicUsize::new(0));
    todo!();
    counter.load(Ordering::Relaxed)
}

/// A read-mostly lookup table shared by every request. Reads take `read()`, which many threads may hold at
/// once; the rare update takes `write()`.
#[derive(Default)]
pub struct Rates {
    by_currency: RwLock<HashMap<String, u32>>,
}

impl Rates {
    /// 2. The rate for `currency`, if known. Take a read lock and copy the value out.
    pub fn get(&self, currency: &str) -> Option<u32> {
        todo!()
    }

    /// 3. Set a rate. Take the write lock; recover from poisoning with `unwrap_or_else(PoisonError::into_inner)`.
    pub fn set(&self, currency: &str, rate: u32) {
        todo!()
    }
}

/// 4. Keep the critical section short: compute OUTSIDE the lock, lock only to push. Each of `threads` threads
///    pushes "worker <i>: <i * i>". Return the lines, sorted.
pub fn collect_lines(threads: usize) -> Vec<String> {
    let lines = Arc::new(Mutex::new(Vec::new()));
    todo!();
    let mut lines = lines.lock().unwrap_or_else(PoisonError::into_inner).clone();
    lines.sort();
    lines
}

/// 5. Async version: a `tokio::sync::Mutex` may be held across `.await` (a std `MutexGuard` must not be).
///    Lock, sleep 1 ms while holding it (simulating a fetch that must happen once), then add `amount`.
pub async fn add_slowly(total: Arc<tokio::sync::Mutex<u32>>, amount: u32) {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_atomically() {
        assert_eq!(atomic_count(8, 1000), 8000);
    }

    #[test]
    fn rates_are_shared() {
        let rates = Arc::new(Rates::default());
        rates.set("EUR", 100);
        let reader = Arc::clone(&rates);
        let seen = thread::spawn(move || reader.get("EUR")).join().ok().flatten();
        assert_eq!(seen, Some(100));
        assert_eq!(rates.get("USD"), None);
    }

    #[test]
    fn collects_from_threads() {
        assert_eq!(collect_lines(3), ["worker 0: 0", "worker 1: 1", "worker 2: 4"]);
    }

    #[tokio::test]
    async fn holds_an_async_lock_across_await() {
        let total = Arc::new(tokio::sync::Mutex::new(0));
        tokio::join!(add_slowly(Arc::clone(&total), 2), add_slowly(Arc::clone(&total), 3));
        assert_eq!(*total.lock().await, 5);
    }
}
