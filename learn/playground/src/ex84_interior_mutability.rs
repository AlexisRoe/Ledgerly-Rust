//! Lesson 84: interior mutability. Run `cargo test ex84`.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

/// 1. Counts calls through `&self`. `Cell<u32>` allows `get`/`set` of a `Copy` value without `&mut`.
pub struct Calls {
    count: Cell<u32>,
}

impl Calls {
    pub fn new() -> Self {
        Self { count: Cell::new(0) }
    }

    /// Records one call and returns the new count.
    pub fn hit(&self) -> u32 {
        todo!()
    }
}

/// 2. Computes squares and remembers them. `RefCell` gives `borrow()` / `borrow_mut()` checked at run time:
///    a second `borrow_mut()` while one is alive panics instead of failing to compile.
pub struct Squares {
    known: RefCell<HashMap<u64, u64>>,
    computed: Cell<u32>,
}

impl Squares {
    pub fn new() -> Self {
        Self { known: RefCell::new(HashMap::new()), computed: Cell::new(0) }
    }

    /// `n * n`, computed once per `n`: look it up with `self.known.borrow()`; if missing, compute it, add 1 to
    /// `computed`, and store it with `self.known.borrow_mut().insert(…)`. Let the first borrow end before the
    /// second starts (copy the value out with `.get(&n).copied()`).
    pub fn square(&self, n: u64) -> u64 {
        todo!()
    }

    pub fn computed(&self) -> u32 {
        self.computed.get()
    }
}

/// 3. The thread-safe version: a log several threads append to through `&self`.
pub struct SharedLog {
    lines: Mutex<Vec<String>>,
}

impl SharedLog {
    pub fn new() -> Self {
        Self { lines: Mutex::new(Vec::new()) }
    }

    /// Append a line. `lock()` returns a `Result` (the lock is "poisoned" if a thread panicked while holding
    /// it); this exercise may recover with `.unwrap_or_else(std::sync::PoisonError::into_inner)`.
    pub fn push(&self, line: &str) {
        todo!()
    }

    pub fn len(&self) -> usize {
        self.lines.lock().unwrap_or_else(std::sync::PoisonError::into_inner).len()
    }
}

/// 4. Built once, on first use, then shared for the rest of the program: `static NAME: OnceLock<String>` and
///    `NAME.get_or_init(|| …)`. Return "ledgerly-" followed by the crate version, `env!("CARGO_PKG_VERSION")`.
pub fn service_name() -> &'static str {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_through_shared_references() {
        let calls = Calls::new();
        let a = &calls;
        let b = &calls;
        assert_eq!(a.hit(), 1);
        assert_eq!(b.hit(), 2);
    }

    #[test]
    fn computes_each_square_once() {
        let squares = Squares::new();
        assert_eq!(squares.square(12), 144);
        assert_eq!(squares.square(12), 144);
        assert_eq!(squares.square(3), 9);
        assert_eq!(squares.computed(), 2);
    }

    #[test]
    fn threads_share_the_log() {
        let log = SharedLog::new();
        std::thread::scope(|scope| {
            for worker in 0..4 {
                let log = &log;
                scope.spawn(move || log.push(&format!("worker {worker}")));
            }
        });
        assert_eq!(log.len(), 4);
    }

    #[test]
    fn initialises_once() {
        let first = service_name();
        let second = service_name();
        assert_eq!(first, "ledgerly-0.1.0");
        assert!(std::ptr::eq(first, second));
    }
}
