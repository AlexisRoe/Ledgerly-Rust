//! Lesson 39: borrowing. Run `cargo test ex39`.
//!
//! The four functions below have parameter types marked `/* ? */`. Replace each with the right one of
//! `Vec<u32>`, `&Vec<u32>` (or better `&[u32]`), `&mut Vec<u32>` — so the tests compile and pass.
//! The tests are fixed; read how they call each function. Until you fix them, the functions use a placeholder
//! type and the tests are commented out. Uncomment the tests module body after you change the signatures.

/// Reads only.
pub fn sum(amounts: &[u32] /* ? keep or change */) -> u32 {
    amounts.iter().sum()
}

/// Appends an amount. The caller keeps using its vector afterwards.
pub fn add(amounts: Vec<u32> /* ? */, amount: u32) {
    let mut amounts = amounts;
    amounts.push(amount);
}

/// Removes every amount above the cap, in place.
pub fn cap(amounts: Vec<u32> /* ? */, max: u32) {
    let mut amounts = amounts;
    amounts.retain(|a| *a <= max);
}

/// Consumes the list and turns it into a report; the caller doesn't need the list afterwards.
pub fn into_report(amounts: Vec<u32> /* ? */) -> String {
    format!("{} amounts", amounts.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn borrows_in_turn() {
        let mut amounts = vec![100, 5000, 250];
        assert_eq!(sum(&amounts), 5350);
        // Uncomment after fixing `add` and `cap`:
        // add(&mut amounts, 50);
        // cap(&mut amounts, 1000);
        // assert_eq!(amounts, vec![100, 250, 50]);
        assert_eq!(into_report(amounts), "3 amounts");
    }
}
