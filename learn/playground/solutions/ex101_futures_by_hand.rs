//! Lesson 101: inside a future, `poll` and `Waker`. Run `cargo test ex101`. No Tokio here: only std.

use std::future::Future;
use std::pin::{Pin, pin};
use std::sync::{Arc, Mutex, PoisonError};
use std::task::{Context, Poll, Wake, Waker};
use std::thread::{self, Thread};
use std::time::Duration;

/// 1. A future that returns `Pending` `remaining` times, then `Ready(number of polls)`. Before each
///    `Pending` it must make sure it gets polled again: call `cx.waker().wake_by_ref()` ("poll me again
///    soon"). Always add 1 to `polls` first.
pub struct YieldTimes {
    remaining: u32,
    polls: u32,
}

impl YieldTimes {
    pub fn new(remaining: u32) -> Self {
        Self { remaining, polls: 0 }
    }
}

impl Future for YieldTimes {
    type Output = u32;

    // `self: Pin<&mut Self>`: YieldTimes holds no references to itself, so it is `Unpin` and its fields can
    // be changed through the Pin as usual (lesson 102).
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<u32> {
        self.polls += 1;
        if self.remaining == 0 {
            return Poll::Ready(self.polls);
        }
        self.remaining -= 1;
        cx.waker().wake_by_ref();
        Poll::Pending
    }
}

/// A waker that unparks the thread running `block_on`.
struct ThreadWaker(Thread);

impl Wake for ThreadWaker {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }
}

/// 2. The smallest executor: poll the future; on `Ready(value)` return it; on `Pending` sleep with
///    `thread::park()` until a waker unparks this thread, then poll again. The setup is done for you.
pub fn block_on<F: Future>(future: F) -> F::Output {
    let mut future = pin!(future);
    let waker = Waker::from(Arc::new(ThreadWaker(thread::current())));
    let mut cx = Context::from_waker(&waker);
    loop {
        match future.as_mut().poll(&mut cx) {
            Poll::Ready(value) => return value,
            Poll::Pending => thread::park(),
        }
    }
}

/// A timer that completes after `duration`. A helper thread sleeps, then marks it done and wakes the stored
/// waker: the shape of every real I/O future (an OS event instead of a thread).
pub struct Delay {
    shared: Arc<Mutex<(bool, Option<Waker>)>>,
}

impl Delay {
    pub fn new(duration: Duration) -> Self {
        let shared = Arc::new(Mutex::new((false, None::<Waker>)));
        let for_thread = Arc::clone(&shared);
        thread::spawn(move || {
            thread::sleep(duration);
            let mut state = for_thread.lock().unwrap_or_else(PoisonError::into_inner);
            state.0 = true;
            if let Some(waker) = state.1.take() {
                waker.wake();
            }
        });
        Self { shared }
    }
}

impl Future for Delay {
    type Output = ();

    /// 3. Lock the state. Done → `Ready(())`. Not done → store `cx.waker().clone()` in the state (so the thread
    ///    can wake us) and return `Pending`. Unlike `YieldTimes`, don't wake yourself: that's the thread's job.
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        let mut state = self.shared.lock().unwrap_or_else(PoisonError::into_inner);
        if state.0 {
            Poll::Ready(())
        } else {
            state.1 = Some(cx.waker().clone());
            Poll::Pending
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use super::*;

    #[test]
    fn polls_until_ready() {
        assert_eq!(block_on(YieldTimes::new(3)), 4);
    }

    #[test]
    fn async_blocks_are_futures_too() {
        let answer = block_on(async {
            let polls = YieldTimes::new(1).await;
            polls * 10
        });
        assert_eq!(answer, 20);
    }

    #[test]
    fn sleeps_without_spinning() {
        let start = Instant::now();
        block_on(Delay::new(Duration::from_millis(30)));
        assert!(start.elapsed() >= Duration::from_millis(30));
    }
}
