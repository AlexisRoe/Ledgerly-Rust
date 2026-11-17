//! Lesson 103: cancellation and structured concurrency. Run `cargo test ex103`.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use tokio::task::JoinSet;
use tokio::time::sleep;

/// 1. Spawn one task per entry into a `JoinSet`: task `i` sleeps `delays_ms[i]` ms, then returns `i`. Collect
///    the ids in the order the tasks FINISH, with `while let Some(result) = set.join_next().await`.
///    (Dropping a JoinSet aborts the tasks still in it: no task outlives its owner.)
pub async fn finish_order(delays_ms: Vec<u64>) -> Vec<usize> {
    let mut set: JoinSet<usize> = JoinSet::new();
    for (index, delay) in delays_ms.into_iter().enumerate() {
        set.spawn(async move {
            sleep(Duration::from_millis(delay)).await;
            index
        });
    }
    let mut order = Vec::new();
    while let Some(result) = set.join_next().await {
        if let Ok(index) = result {
            order.push(index);
        }
    }
    order
}

/// 2. Spawn a task that sleeps 1 s and then sets `finished` to true. After 5 ms, `abort()` it, await the
///    handle, and return `(finished, error.is_cancelled())`. The flag stays false: the task was dropped at its
///    `.await`, and the code after it never ran.
pub async fn abort_early() -> (bool, bool) {
    let finished = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&finished);
    let handle = tokio::spawn(async move {
        sleep(Duration::from_secs(1)).await;
        flag.store(true, Ordering::SeqCst);
    });
    sleep(Duration::from_millis(5)).await;
    handle.abort();
    let cancelled = match handle.await {
        Ok(()) => false,
        Err(error) => error.is_cancelled(),
    };
    (finished.load(Ordering::SeqCst), cancelled)
}

/// 3. `tokio::time::timeout(limit, future)` returns `Err(Elapsed)` if the limit passes first (and drops the
///    future). Map it to an Option.
pub async fn within<F: std::future::Future<Output = u32>>(limit: Duration, future: F) -> Option<u32> {
    tokio::time::timeout(limit, future).await.ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn collects_as_tasks_finish() {
        assert_eq!(finish_order(vec![60, 10, 35]).await, [1, 2, 0]);
    }

    #[tokio::test]
    async fn aborted_tasks_stop_at_their_await() {
        assert_eq!(abort_early().await, (false, true));
    }

    #[tokio::test]
    async fn times_out() {
        assert_eq!(within(Duration::from_millis(50), async { 7 }).await, Some(7));
        let slow = async {
            sleep(Duration::from_millis(200)).await;
            1
        };
        assert_eq!(within(Duration::from_millis(10), slow).await, None);
    }
}
