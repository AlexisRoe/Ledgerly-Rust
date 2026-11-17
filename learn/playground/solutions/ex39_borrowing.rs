//! Lesson 39: borrowing. Run `cargo test ex39`.

/// Reads only.
pub fn sum(amounts: &[u32]) -> u32 {
    amounts.iter().sum()
}

/// Appends an amount. The caller keeps using its vector afterwards.
pub fn add(amounts: &mut Vec<u32>, amount: u32) {
    amounts.push(amount);
}

/// Removes every amount above the cap, in place.
pub fn cap(amounts: &mut Vec<u32>, max: u32) {
    amounts.retain(|a| *a <= max);
}

/// Consumes the list and turns it into a report.
pub fn into_report(amounts: Vec<u32>) -> String {
    format!("{} amounts", amounts.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn borrows_in_turn() {
        let mut amounts = vec![100, 5000, 250];
        assert_eq!(sum(&amounts), 5350);
        add(&mut amounts, 50);
        cap(&mut amounts, 1000);
        assert_eq!(amounts, vec![100, 250, 50]);
        assert_eq!(into_report(amounts), "3 amounts");
    }
}
