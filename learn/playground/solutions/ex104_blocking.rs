//! Lesson 104: blocking work inside async code. Run `cargo test ex104`.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::Duration;

/// CPU-heavy on purpose: counts primes below `limit` by trial division. No `.await` anywhere, so while it
/// runs on an async worker thread, nothing else can run on that thread.
pub fn count_primes(limit: u64) -> u64 {
    (2..limit).filter(|&n| (2..).take_while(|d| d * d <= n).all(|d| n % d != 0)).count() as u64
}

/// 1. Run `count_primes` on Tokio's blocking thread pool: `tokio::task::spawn_blocking(move || …)`, then
///    `.await` the handle. A panic in the closure arrives as `Err(JoinError)`; return 0 then.
pub async fn primes_below(limit: u64) -> u64 {
    tokio::task::spawn_blocking(move || count_primes(limit)).await.unwrap_or(0)
}

/// 2. While `primes_below(limit)` runs, a ticker task should keep ticking. Spawn a task that adds 1 to `ticks`
///    every millisecond until `stop` is true; await `primes_below(limit)`; set `stop`; await the ticker and
///    return the tick count.
///    The test runs this on a SINGLE-threaded runtime. Try replacing `primes_below(limit).await` with a direct
///    `count_primes(limit)` call: the ticker never gets a turn, and the test fails.
pub async fn ticks_while_counting(limit: u64) -> u32 {
    let ticks = Arc::new(AtomicU32::new(0));
    let stop = Arc::new(AtomicBool::new(false));
    let ticker = {
        let ticks = Arc::clone(&ticks);
        let stop = Arc::clone(&stop);
        tokio::spawn(async move {
            while !stop.load(Ordering::SeqCst) {
                ticks.fetch_add(1, Ordering::SeqCst);
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
        })
    };
    let _ = primes_below(limit).await;
    stop.store(true, Ordering::SeqCst);
    let _ = ticker.await;
    ticks.load(Ordering::SeqCst)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn counts_on_the_blocking_pool() {
        assert_eq!(primes_below(100).await, 25);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn the_runtime_stays_responsive() {
        let ticks = ticks_while_counting(300_000).await;
        assert!(ticks > 1, "ticked only {ticks} times");
    }
}
