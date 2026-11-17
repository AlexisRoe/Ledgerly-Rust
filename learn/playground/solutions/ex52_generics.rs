//! Lesson 52: generics with trait bounds. Run `cargo test ex52`.

use std::fmt::Display;

pub fn same<T: PartialEq>(a: T, b: T) -> bool {
    a == b
}

pub fn bracketed<T: Display>(items: &[T]) -> String {
    items.iter().map(|item| format!("[{item}]")).collect::<Vec<_>>().join(" ")
}

pub fn largest<T>(items: &[T]) -> Option<T>
where
    T: PartialOrd + Copy,
{
    let mut best = *items.first()?;
    for &item in items {
        if item > best {
            best = item;
        }
    }
    Some(best)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generic_functions() {
        assert!(same(3, 3));
        assert!(!same("a", "b"));
        assert_eq!(bracketed(&[1, 2]), "[1] [2]");
        assert_eq!(bracketed(&["EUR"]), "[EUR]");
        assert_eq!(largest(&[3, 9, 2]), Some(9));
        assert_eq!(largest::<u8>(&[]), None);
    }
}
