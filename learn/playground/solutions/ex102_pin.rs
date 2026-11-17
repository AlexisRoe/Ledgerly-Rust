//! Lesson 102: `Pin`, just enough. Run `cargo test ex102`.

use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use tokio::time::sleep;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// 1. Two async blocks have two different (anonymous) types, so an `if` can't return one or the other…
///    unless both are boxed into the same `BoxFuture`. Return `async { "cache" }` when `cached`, else an
///    async block that sleeps 1 ms and returns "db". Wrap each in `Box::pin(…)`.
pub fn lookup(cached: bool) -> BoxFuture<'static, &'static str> {
    if cached {
        Box::pin(async { "cache" })
    } else {
        Box::pin(async {
            sleep(Duration::from_millis(1)).await;
            "db"
        })
    }
}

/// 2. A list of different futures, all boxed. Await them one after another and collect the outputs.
pub async fn run_in_order(futures: Vec<BoxFuture<'static, u32>>) -> Vec<u32> {
    let mut outputs = Vec::new();
    for future in futures {
        outputs.push(future.await);
    }
    outputs
}

/// 3. Keep ONE long future alive across loop iterations. `select!` needs `&mut` to a pinned future to poll it
///    without consuming it, so pin it first: `let mut deadline = std::pin::pin!(sleep(total));`. Then loop:
///    `select! { () = &mut deadline => return ticks, () = sleep(every) => ticks += 1 }`.
pub async fn ticks_until(total: Duration, every: Duration) -> u32 {
    let mut ticks = 0;
    let mut deadline = std::pin::pin!(sleep(total));
    loop {
        tokio::select! {
            () = &mut deadline => return ticks,
            () = sleep(every) => ticks += 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn boxes_different_futures() {
        assert_eq!(lookup(true).await, "cache");
        assert_eq!(lookup(false).await, "db");
    }

    #[tokio::test]
    async fn runs_a_list_of_futures() {
        let futures: Vec<BoxFuture<'static, u32>> = vec![
            Box::pin(async { 1 }),
            Box::pin(async {
                sleep(Duration::from_millis(1)).await;
                2
            }),
        ];
        assert_eq!(run_in_order(futures).await, [1, 2]);
    }

    #[tokio::test]
    async fn one_deadline_many_ticks() {
        let ticks = ticks_until(Duration::from_millis(40), Duration::from_millis(5)).await;
        assert!((2..=8).contains(&ticks), "ticked {ticks} times");
    }
}
