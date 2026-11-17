//! Lesson 40: borrow-checker errors. Run `cargo test ex40`.

/// E0502: `first` still borrows `values` when `push` needs `&mut`. Fix: copy the number out first.
pub fn first_then_push(mut values: Vec<u32>, extra: u32) -> (u32, usize) {
    let first = values[0]; // u32 is Copy: no borrow is kept
    values.push(extra);
    (first, values.len())
}

/// E0596: `make_ascii_uppercase` needs `&mut`, so the binding must be `mut`.
pub fn upper(label: String) -> String {
    let mut result = label;
    result.make_ascii_uppercase();
    result
}

/// Return the owned String, not a borrow of it.
pub fn greeting(name: &str) -> String {
    format!("Hi {name}")
}

/// E0499: one `&mut` at a time. Let each borrow end before the next.
pub fn bump_both(counters: &mut Vec<u32>) {
    counters[0] += 1;
    counters[1] += 1;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_and_length() {
        assert_eq!(first_then_push(vec![7, 8], 9), (7, 3));
    }

    #[test]
    fn uppercases() {
        assert_eq!(upper("bahn".to_owned()), "BAHN");
    }

    #[test]
    fn greets() {
        assert_eq!(greeting("Sam"), "Hi Sam");
    }

    #[test]
    fn bumps() {
        let mut c = vec![1, 1];
        bump_both(&mut c);
        assert_eq!(c, vec![2, 2]);
    }
}
