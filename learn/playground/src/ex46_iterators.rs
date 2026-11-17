//! Lesson 46: iterators. Run `cargo test ex46`. Use iterator chains, no `for` loops.

#[derive(Debug, Clone, PartialEq)]
pub struct Expense {
    pub merchant: String,
    pub cents: u32,
    pub approved: bool,
}

/// TS: `expenses.map(e => e.merchant)`
pub fn merchants(expenses: &[Expense]) -> Vec<String> {
    todo!()
}

/// TS: `expenses.filter(e => e.approved).reduce((sum, e) => sum + e.cents, 0)`
pub fn approved_total(expenses: &[Expense]) -> u32 {
    todo!()
}

/// TS: `expenses.some(e => e.cents > 100_000)`
pub fn has_big_one(expenses: &[Expense]) -> bool {
    todo!()
}

/// TS: `expenses.find(e => !e.approved)?.merchant`   → None if there is none
pub fn first_open_merchant(expenses: &[Expense]) -> Option<&str> {
    todo!()
}

/// TS: `expenses.map((e, i) => `${i + 1}. ${e.merchant}`)`
pub fn numbered(expenses: &[Expense]) -> Vec<String> {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Vec<Expense> {
        vec![
            Expense { merchant: "Bahn".to_owned(), cents: 9900, approved: true },
            Expense { merchant: "Café".to_owned(), cents: 450, approved: false },
            Expense { merchant: "Hotel".to_owned(), cents: 15000, approved: true },
        ]
    }

    #[test]
    fn maps() {
        assert_eq!(merchants(&sample()), vec!["Bahn", "Café", "Hotel"]);
    }

    #[test]
    fn totals() {
        assert_eq!(approved_total(&sample()), 24900);
    }

    #[test]
    fn some() {
        assert!(!has_big_one(&sample()));
    }

    #[test]
    fn finds() {
        assert_eq!(first_open_merchant(&sample()), Some("Café"));
    }

    #[test]
    fn numbers() {
        assert_eq!(numbered(&sample()), vec!["1. Bahn", "2. Café", "3. Hotel"]);
    }
}
