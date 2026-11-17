//! Lesson 100: channels and the actor pattern. Run `cargo test ex100`.

use tokio::sync::{mpsc, oneshot, watch};

/// 1. Producer/consumer: spawn one task per number in `items` that sends `n * 2` into an mpsc channel
///    (`tx.clone()` per task; `send(…).await`). Drop the original `tx`, then receive until `recv()` returns
///    `None` (all senders gone) and return the sum.
pub async fn doubled_sum(items: Vec<u64>) -> u64 {
    let (tx, mut rx) = mpsc::channel::<u64>(8);
    for n in items {
        let tx = tx.clone();
        tokio::spawn(async move {
            let _ = tx.send(n * 2).await;
        });
    }
    drop(tx);
    let mut sum = 0;
    while let Some(value) = rx.recv().await {
        sum += value;
    }
    sum
}

/// What the counter actor understands. `Get` carries a oneshot sender: the reply channel.
pub enum Command {
    Add(u64),
    Get(oneshot::Sender<u64>),
}

/// The actor: owns its state (no lock!) and handles one command at a time.
/// 2. Loop over `commands.recv().await`; `Add` adds, `Get` replies with `reply.send(total)`.
///    The loop ends when every handle is dropped.
pub async fn counter_actor(mut commands: mpsc::Receiver<Command>) {
    let mut total = 0;
    while let Some(command) = commands.recv().await {
        match command {
            Command::Add(amount) => total += amount,
            Command::Get(reply) => {
                let _ = reply.send(total);
            }
        }
    }
}

/// The cheap, cloneable handle other code uses to talk to the actor.
#[derive(Clone)]
pub struct Counter {
    commands: mpsc::Sender<Command>,
}

impl Counter {
    pub fn start() -> Self {
        let (commands, receiver) = mpsc::channel(16);
        tokio::spawn(counter_actor(receiver));
        Self { commands }
    }

    pub async fn add(&self, amount: u64) {
        let _ = self.commands.send(Command::Add(amount)).await;
    }

    /// 3. Create a oneshot channel, send `Command::Get(reply_tx)`, await `reply_rx`. 0 if the actor is gone.
    pub async fn get(&self) -> u64 {
        let (reply, answer) = oneshot::channel();
        if self.commands.send(Command::Get(reply)).await.is_err() {
            return 0;
        }
        answer.await.unwrap_or(0)
    }
}

/// 4. A worker that counts ticks until shutdown, like Ledgerly's `Shutdown`: loop with `tokio::select!` over
///    `stop.changed()` (end the loop when `*stop.borrow()` is true) and a 1 ms sleep (count a tick).
///    Return the number of ticks.
pub async fn ticker(mut stop: watch::Receiver<bool>) -> u32 {
    let mut ticks = 0;
    loop {
        tokio::select! {
            changed = stop.changed() => {
                if changed.is_err() || *stop.borrow() {
                    return ticks;
                }
            }
            () = tokio::time::sleep(std::time::Duration::from_millis(1)) => ticks += 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[tokio::test]
    async fn fans_in() {
        assert_eq!(doubled_sum(vec![1, 2, 3, 4]).await, 20);
    }

    #[tokio::test]
    async fn the_actor_owns_its_state() {
        let counter = Counter::start();
        let other = counter.clone();
        counter.add(5).await;
        other.add(7).await;
        assert_eq!(counter.get().await, 12);
    }

    #[tokio::test]
    async fn stops_on_signal() {
        let (stop, receiver) = watch::channel(false);
        let worker = tokio::spawn(ticker(receiver));
        tokio::time::sleep(Duration::from_millis(20)).await;
        let _ = stop.send(true);
        let ticks = worker.await.unwrap_or(0);
        assert!(ticks > 0, "ticked {ticks} times");
    }
}
