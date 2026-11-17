//! Lesson 89: implementing `Iterator` yourself. Run `cargo test ex89`.

/// 1. The Fibonacci numbers 0, 1, 1, 2, 3, 5, … Implement `next` only; `take`, `filter`, `sum` and every
///    other adapter come for free.
pub struct Fibonacci {
    current: u64,
    next: u64,
}

impl Fibonacci {
    pub fn new() -> Self {
        Self { current: 0, next: 1 }
    }
}

impl Iterator for Fibonacci {
    type Item = u64;

    fn next(&mut self) -> Option<u64> {
        let value = self.current;
        self.current = self.next;
        self.next += value;
        Some(value)
    }
}

/// 2. Pages of `size` items, borrowed from a slice: `&'a [T]` per page, the last one possibly shorter.
///    Hint: `self.items.split_at(self.size.min(self.items.len()))`.
pub struct Pages<'a, T> {
    items: &'a [T],
    size: usize,
}

pub fn pages<T>(items: &[T], size: usize) -> Pages<'_, T> {
    Pages { items, size: size.max(1) }
}

impl<'a, T> Iterator for Pages<'a, T> {
    type Item = &'a [T];

    fn next(&mut self) -> Option<&'a [T]> {
        if self.items.is_empty() {
            return None;
        }
        let (page, rest) = self.items.split_at(self.size.min(self.items.len()));
        self.items = rest;
        Some(page)
    }
}

/// A list of expense lines. 3. Make `for line in &ledger` work by implementing `IntoIterator` for
/// `&Ledger`: return `self.lines.iter()`.
pub struct Ledger {
    lines: Vec<String>,
}

impl<'a> IntoIterator for &'a Ledger {
    type Item = &'a String;
    type IntoIter = std::slice::Iter<'a, String>;

    fn into_iter(self) -> Self::IntoIter {
        self.lines.iter()
    }
}

/// 4. No struct at all: `std::iter::from_fn(move || …)` turns a closure into an iterator. Yield `start`,
///    then halve it (integer division) while it's above 0: 20, 10, 5, 2, 1.
pub fn halvings(start: u32) -> impl Iterator<Item = u32> {
    let mut value = start;
    std::iter::from_fn(move || {
        if value == 0 {
            return None;
        }
        let current = value;
        value /= 2;
        Some(current)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fibonacci_gets_every_adapter() {
        let first: Vec<u64> = Fibonacci::new().take(8).collect();
        assert_eq!(first, [0, 1, 1, 2, 3, 5, 8, 13]);
        let even: Vec<u64> = Fibonacci::new().filter(|n| n % 2 == 0).take(3).collect();
        assert_eq!(even, [0, 2, 8]);
    }

    #[test]
    fn pages_borrow_the_slice() {
        let items = [1, 2, 3, 4, 5];
        let all: Vec<&[i32]> = pages(&items, 2).collect();
        assert_eq!(all, [&[1, 2][..], &[3, 4], &[5]]);
        assert_eq!(pages(&[] as &[i32], 3).count(), 0);
    }

    #[test]
    fn for_loops_over_a_ledger() {
        let ledger = Ledger { lines: vec!["taxi".to_owned(), "hotel".to_owned()] };
        let mut seen = Vec::new();
        for line in &ledger {
            seen.push(line.as_str());
        }
        assert_eq!(seen, ["taxi", "hotel"]);
    }

    #[test]
    fn halves_until_zero() {
        assert_eq!(halvings(20).collect::<Vec<_>>(), [20, 10, 5, 2, 1]);
        assert_eq!(halvings(0).count(), 0);
    }
}
