//! Lesson 94: comparison and operator traits. Run `cargo test ex94`.

use std::cmp::Ordering;
use std::ops::Add;

/// Derived ordering follows declaration order: Low < Normal < High.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Priority {
    Low,
    Normal,
    High,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Task {
    pub priority: Priority,
    pub title: String,
}

/// 1. A hand-written total order: HIGHER priority first, then title A→Z. Hint: compare
///    `other.priority` with `self.priority` (reversed), then chain `.then_with(|| self.title.cmp(&other.title))`.
impl Ord for Task {
    fn cmp(&self, other: &Self) -> Ordering {
        other.priority.cmp(&self.priority).then_with(|| self.title.cmp(&other.title))
    }
}

/// `PartialOrd` must agree with `Ord`; the standard way is to delegate.
impl PartialOrd for Task {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// An amount in cents of ONE currency, so `+` always has an answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct Cents(pub u64);

/// 2. `a + b` for Cents.
impl Add for Cents {
    type Output = Cents;

    fn add(self, other: Cents) -> Cents {
        Cents(self.0 + other.0)
    }
}

/// 3. `.sum()` over Cents. Fold from `Cents::default()` with `+`.
impl std::iter::Sum for Cents {
    fn sum<I: Iterator<Item = Cents>>(iter: I) -> Cents {
        iter.fold(Cents::default(), |total, cents| total + cents)
    }
}

/// An amount with a currency. NO `Add`, NO `Ord`: two currencies have no sum and no order. Like Ledgerly's
/// `Money`, it offers methods that can say no.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Amount {
    pub cents: u64,
    pub currency: &'static str,
}

impl Amount {
    /// 4. `Some(sum)` in the same currency, `None` if the currencies differ.
    pub fn checked_add(self, other: Amount) -> Option<Amount> {
        (self.currency == other.currency).then(|| Amount { cents: self.cents + other.cents, currency: self.currency })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task(priority: Priority, title: &str) -> Task {
        Task { priority, title: title.to_owned() }
    }

    #[test]
    fn sorts_tasks() {
        let mut tasks = vec![
            task(Priority::Low, "b"),
            task(Priority::High, "z"),
            task(Priority::Low, "a"),
            task(Priority::Normal, "m"),
        ];
        tasks.sort();
        let titles: Vec<&str> = tasks.iter().map(|t| t.title.as_str()).collect();
        assert_eq!(titles, ["z", "m", "a", "b"]);
        assert!(Priority::High > Priority::Low);
    }

    #[test]
    fn adds_and_sums_cents() {
        assert_eq!(Cents(250) + Cents(100), Cents(350));
        let total: Cents = [Cents(1), Cents(2), Cents(3)].into_iter().sum();
        assert_eq!(total, Cents(6));
    }

    #[test]
    fn refuses_to_mix_currencies() {
        let eur = Amount { cents: 100, currency: "EUR" };
        let usd = Amount { cents: 100, currency: "USD" };
        assert_eq!(eur.checked_add(eur), Some(Amount { cents: 200, currency: "EUR" }));
        assert_eq!(eur.checked_add(usd), None);
    }
}
