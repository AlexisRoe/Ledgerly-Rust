//! Lesson 19: tuples, arrays, slices, Vec. Run `cargo test ex19`.

/// Every category name, fixed at compile time. Fill in the five names in this order:
/// "TRAVEL", "MEALS", "OFFICE", "SOFTWARE", "OTHER". (The type already says there are five.)
pub const CATEGORIES: [&str; 5] = ["TRAVEL", "MEALS", "OFFICE", "SOFTWARE", "OTHER"];

/// Sum of all amounts in the slice. Works for a Vec, an array, or part of either.
/// Hint: `for amount in amounts { … }` gives you `&u32`; `*amount` reads the number.
pub fn sum(amounts: &[u32]) -> u32 {
    let mut total = 0;
    for amount in amounts {
        total += *amount;
    }
    total
}

/// Returns the first amount, or None for an empty slice. Don't use `amounts[0]`.
pub fn first(amounts: &[u32]) -> Option<u32> {
    amounts.first().copied()
}

/// Collects every amount above `threshold` into a new Vec, in order.
pub fn above(amounts: &[u32], threshold: u32) -> Vec<u32> {
    let mut result: Vec<u32> = Vec::new();
    for amount in amounts {
        if *amount > threshold {
            result.push(*amount);
        }
    }
    result
}

/// Returns (smallest, largest) as a tuple. The slice is never empty in the tests.
/// Hint: start both with the first element, then loop.
pub fn min_max(amounts: &[u32]) -> (u32, u32) {
    let mut low = amounts[0];
    let mut high = amounts[0];
    for &amount in amounts {
        if amount < low {
            low = amount;
        }
        if amount > high {
            high = amount;
        }
    }
    (low, high)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn categories_are_complete() {
        assert_eq!(CATEGORIES[1], "MEALS");
        assert_eq!(CATEGORIES[4], "OTHER");
    }

    #[test]
    fn sums_vecs_arrays_and_parts() {
        let v = vec![100, 250, 50];
        assert_eq!(sum(&v), 400);
        assert_eq!(sum(&[1, 2, 3]), 6);
        assert_eq!(sum(&v[1..]), 300);
    }

    #[test]
    fn first_is_safe() {
        assert_eq!(first(&[7, 8]), Some(7));
        assert_eq!(first(&[]), None);
    }

    #[test]
    fn above_filters() {
        assert_eq!(above(&[100, 2000, 30, 5000], 1000), vec![2000, 5000]);
    }

    #[test]
    fn min_and_max() {
        let (low, high) = min_max(&[40, 10, 90, 20]);
        assert_eq!((low, high), (10, 90));
    }
}
