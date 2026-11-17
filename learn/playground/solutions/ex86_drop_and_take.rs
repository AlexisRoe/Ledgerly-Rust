//! Lesson 86: Drop, RAII and `mem::take` / `mem::replace`. Run `cargo test ex86`.

use std::cell::RefCell;

pub struct Scope<'a> {
    log: &'a RefCell<Vec<String>>,
    name: &'static str,
}

impl<'a> Scope<'a> {
    pub fn enter(log: &'a RefCell<Vec<String>>, name: &'static str) -> Self {
        log.borrow_mut().push(format!("enter {name}"));
        Self { log, name }
    }
}

impl Drop for Scope<'_> {
    fn drop(&mut self) {
        self.log.borrow_mut().push(format!("exit {}", self.name));
    }
}

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

    pub fn flush(&mut self) -> Vec<String> {
        std::mem::take(&mut self.items)
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Connection {
    Idle,
    Open { peer: String },
    Closed,
}

impl Connection {
    pub fn close(&mut self) -> Option<String> {
        match std::mem::replace(self, Connection::Closed) {
            Connection::Open { peer } => Some(peer),
            Connection::Idle | Connection::Closed => None,
        }
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
