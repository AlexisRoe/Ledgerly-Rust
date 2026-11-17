//! Lesson 52: generics with trait bounds. Run `cargo test ex52`.
//!
//! The three functions below are commented out because they don't compile without bounds. Uncomment one at a
//! time, run `cargo test ex52`, read the error (it names the missing trait), and add the bound.

use std::fmt::Display;

// /// Returns true if both values are equal.
// pub fn same<T>(a: T, b: T) -> bool {
//     a == b
// }

// /// Formats every item as "[item]" and joins them with spaces.
// pub fn bracketed<T>(items: &[T]) -> String {
//     items.iter().map(|item| format!("[{item}]")).collect::<Vec<_>>().join(" ")
// }

// /// Returns the largest item. Items are small and copyable. Use a `where` clause for this one.
// pub fn largest<T>(items: &[T]) -> Option<T> {
//     let mut best = *items.first()?;
//     for &item in items {
//         if item > best {
//             best = item;
//         }
//     }
//     Some(best)
// }

#[cfg(test)]
mod tests {
    use super::*;

    // Uncomment together with the functions.
    // #[test]
    // fn generic_functions() {
    //     assert!(same(3, 3));
    //     assert!(!same("a", "b"));
    //     assert_eq!(bracketed(&[1, 2]), "[1] [2]");
    //     assert_eq!(bracketed(&["EUR"]), "[EUR]");
    //     assert_eq!(largest(&[3, 9, 2]), Some(9));
    //     assert_eq!(largest::<u8>(&[]), None);
    // }
}
