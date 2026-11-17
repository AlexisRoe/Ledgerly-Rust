//! Lesson 86: Drop, RAII and `mem::take` / `mem::replace`. Run `cargo test ex86`.

use std::cell::RefCell;

/// 1. A guard: creating it logs "enter <name>", and dropping it logs "exit <name>", however the scope ends
///    (normal end, `return`, `?`, panic). TS has no equivalent; the closest is `try { … } finally { … }`, or
///    `using` (explicit resource management).
pub struct Scope<'a> {
    log: &'a RefCell<Vec<String>>,
    name: &'static str,
}

impl<'a> Scope<'a> {
    pub fn enter(log: &'a RefCell<Vec<String>>, name: &'static str) -> Self {
        todo!()
    }
}

// Write `impl Drop for Scope<'_> { fn drop(&mut self) { … } }` here.

/// 2. A batch that hands its items over and starts again empty.
pub struct Batch {
    items: Vec<String>,
}

impl Batch {
    pub fn new() -> Self {
        Self { items: Vec::new() }
    }

    pub fn add(&mut self, item: &str) {
        self.items.push(item.to_owned());
    }

    /// Return all items and leave the batch empty. You only have `&mut self`, so you can't move
    /// `self.items` out: `std::mem::take(&mut self.items)` swaps in `Vec::default()` and returns the old one.
    pub fn flush(&mut self) -> Vec<String> {
        todo!()
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Connection {
    Idle,
    Open { peer: String },
    Closed,
}

impl Connection {
    /// 3. Close the connection and return the peer it was open to (`None` if it wasn't open). Use
    ///    `std::mem::replace(self, Connection::Closed)` to get the old state *by value*, then match on it.
    ///    An Idle connection becomes Closed too.
    pub fn close(&mut self) -> Option<String> {
        todo!()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn work(log: &RefCell<Vec<String>>, fail: bool) -> Result<(), String> {
        let _outer = Scope::enter(log, "request");
        let _inner = Scope::enter(log, "query");
        if fail {
            return Err("db down".to_owned());
        }
        Ok(())
    }

    #[test]
    fn guards_run_on_every_exit() {
        let log = RefCell::new(Vec::new());
        let _ = work(&log, true);
        assert_eq!(*log.borrow(), ["enter request", "enter query", "exit query", "exit request"]);
    }

    #[test]
    fn flush_empties_the_batch() {
        let mut batch = Batch::new();
        batch.add("a");
        batch.add("b");
        assert_eq!(batch.flush(), ["a", "b"]);
        assert!(batch.flush().is_empty());
    }

    #[test]
    fn close_returns_the_peer() {
        let mut open = Connection::Open { peer: "db".to_owned() };
        assert_eq!(open.close(), Some("db".to_owned()));
        assert_eq!(open, Connection::Closed);
        let mut idle = Connection::Idle;
        assert_eq!(idle.close(), None);
        assert_eq!(idle, Connection::Closed);
    }
}
