//! Lesson 63: Tokio. Run `cargo test ex63`.

use std::time::Duration;

use tokio::time::sleep;

/// Pretends to look up a user's name (takes 50 ms).
pub async fn user_name(id: u32) -> String {
    sleep(Duration::from_millis(50)).await;
    format!("user-{id}")
}

/// Pretends to look up a team's name (takes 50 ms).
pub async fn team_name(id: u32) -> String {
    sleep(Duration::from_millis(50)).await;
    format!("team-{id}")
}

/// 1. Return "<user> in <team>", running both lookups CONCURRENTLY with `tokio::join!`.
///    (The test fails if it takes 100 ms or more, which is what two sequential awaits would take.)
pub async fn label(user: u32, team: u32) -> String {
    todo!()
}

/// 2. Run `work` but give up after `limit`: return Some(result) or None on timeout. Use `tokio::select!`
///    with `sleep(limit)` as the other branch.
pub async fn with_timeout(work: impl std::future::Future<Output = u32>, limit: Duration) -> Option<u32> {
    todo!()
}

/// 3. Spawn `count` background tasks that each return their index * 10, then await all handles and sum the
///    results. `tokio::spawn(async move { … })` returns a JoinHandle; `.await` on it gives Result<T, JoinError>.
pub async fn spawn_and_sum(count: u32) -> u32 {
    let mut handles: Vec<tokio::task::JoinHandle<u32>> = Vec::new();
    for index in 0..count {
        todo!()
    }
    let mut total = 0;
    for handle in handles {
        total += handle.await.unwrap_or(0);
    }
    total
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use super::*;

    #[tokio::test]
    async fn joins_concurrently() {
        let start = Instant::now();
        assert_eq!(label(1, 2).await, "user-1 in team-2");
        assert!(start.elapsed() < Duration::from_millis(95), "took {:?}", start.elapsed());
    }

    #[tokio::test]
    async fn times_out() {
        let slow = async {
            sleep(Duration::from_millis(200)).await;
            1
        };
        assert_eq!(with_timeout(slow, Duration::from_millis(20)).await, None);
        assert_eq!(with_timeout(async { 7 }, Duration::from_millis(20)).await, Some(7));
    }

    #[tokio::test]
    async fn spawns() {
        assert_eq!(spawn_and_sum(4).await, 60);
    }
}
