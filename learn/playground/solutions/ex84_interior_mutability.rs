//! Lesson 84: interior mutability. Run `cargo test ex84`.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock, PoisonError};

pub struct Calls {
    count: Cell<u32>,
}

impl Calls {
    pub fn new() -> Self {
        Self { count: Cell::new(0) }
    }

    pub fn hit(&self) -> u32 {
        self.count.set(self.count.get() + 1);
        self.count.get()
    }
}

pub struct Squares {
    known: RefCell<HashMap<u64, u64>>,
    computed: Cell<u32>,
}

impl Squares {
    pub fn new() -> Self {
        Self { known: RefCell::new(HashMap::new()), computed: Cell::new(0) }
    }

    pub fn square(&self, n: u64) -> u64 {
        if let Some(known) = self.known.borrow().get(&n).copied() {
            return known;
        }
        let value = n * n;
        self.computed.set(self.computed.get() + 1);
        self.known.borrow_mut().insert(n, value);
        value
    }

    pub fn computed(&self) -> u32 {
        self.computed.get()
    }
}

pub struct SharedLog {
    lines: Mutex<Vec<String>>,
}

impl SharedLog {
    pub fn new() -> Self {
        Self { lines: Mutex::new(Vec::new()) }
    }

    pub fn push(&self, line: &str) {
        self.lines.lock().unwrap_or_else(PoisonError::into_inner).push(line.to_owned());
    }

    pub fn len(&self) -> usize {
        self.lines.lock().unwrap_or_else(PoisonError::into_inner).len()
    }
}

pub fn service_name() -> &'static str {
    static NAME: OnceLock<String> = OnceLock::new();
    NAME.get_or_init(|| format!("ledgerly-{}", env!("CARGO_PKG_VERSION")))
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
