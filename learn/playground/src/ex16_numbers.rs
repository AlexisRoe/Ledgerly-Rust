//! Lesson 16: numbers. Run `cargo test ex16`.

/// Adds two receipt counts. If the sum doesn't fit in a u32, return `None` instead of overflowing.
/// Hint: one method call.
pub fn total_receipts(accepted: u32, pending: u32) -> Option<u32> {
    todo!()
}

/// How many more rows may still be exported. Never below zero, even if `sent` is already above `cap`.
/// Hint: look at the `saturating_*` methods.
pub fn room_left(cap: u32, sent: u32) -> u32 {
    todo!()
}

/// Converts a length (usize) into a page size (u32). Lengths that don't fit become `u32::MAX`.
/// Don't use `as`. Hint: `u32::try_from(...)` returns a Result; `.unwrap_or(...)` picks a fallback.
pub fn page_size(len: usize) -> u32 {
    todo!()
}

/// Adds an amount in cents (u32) to a running total in cents (u64), and returns the new total.
/// The two types differ: convert the u32 without losing anything.
pub fn add_to_total(total: u64, cents: u32) -> u64 {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn totals_and_overflow() {
        assert_eq!(total_receipts(2, 3), Some(5));
        assert_eq!(total_receipts(u32::MAX, 1), None);
    }

    #[test]
    fn room_never_negative() {
        assert_eq!(room_left(100, 40), 60);
        assert_eq!(room_left(100, 140), 0);
    }

    #[test]
    fn page_size_clamps() {
        assert_eq!(page_size(20), 20);
        assert_eq!(page_size(usize::MAX), u32::MAX);
    }

    #[test]
    fn widening_is_lossless() {
        assert_eq!(add_to_total(10_000_000_000, 250), 10_000_000_250);
    }
}
