//! Lesson 21: impl blocks. Run `cargo test ex21`.

/// Counts receipts for one expense, with a cap.
pub struct Counter {
    count: u32,
    cap: u32,
}

impl Counter {
    /// Associated function: a new counter at zero with the given cap.
    pub fn new(cap: u32) -> Self {
        Self { count: 0, cap }
    }

    /// Reads: the current count.
    pub fn count(&self) -> u32 {
        self.count
    }

    /// Modifies: adds one if below the cap and returns true; at the cap it changes nothing and returns false.
    pub fn add(&mut self) -> bool {
        if self.count < self.cap {
            self.count += 1;
            true
        } else {
            false
        }
    }

    /// Consumes the counter and returns a summary like "3/5". After this call the counter is gone.
    pub fn finish(self) -> String {
        format!("{}/{}", self.count, self.cap)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_up_to_the_cap() {
        let mut c = Counter::new(2);
        assert_eq!(c.count(), 0);
        assert!(c.add());
        assert!(c.add());
        assert!(!c.add());
        assert_eq!(c.count(), 2);
        assert_eq!(c.finish(), "2/2");
        // c.count();  // uncomment: E0382 "borrow of moved value" — `finish` took the counter
    }
}
