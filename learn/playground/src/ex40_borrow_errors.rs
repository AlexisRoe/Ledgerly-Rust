//! Lesson 40: borrow-checker errors. Run `cargo test ex40`.
//!
//! Each function compiles as shipped but returns a wrong result. For each:
//!   1. replace the line marked PLACEHOLDER with the commented-out line under it;
//!   2. predict the error code, then run `cargo test ex40` and compare;
//!   3. fix it without `.clone()` (reorder, borrow, add `mut`, or return an owned value).

/// Should return (first element, new length) after pushing `extra`.
pub fn first_then_push(mut values: Vec<u32>, extra: u32) -> (u32, usize) {
    let first = &values[0];
    let result = (0, 0); // PLACEHOLDER
    // values.push(extra); let result = (*first, values.len());
    result
}

/// Should return the label in upper case.
pub fn upper(label: String) -> String {
    let result = String::new(); // PLACEHOLDER
    // let result = label; result.make_ascii_uppercase();
    result
}

/// Should return a greeting. As written (after uncommenting) it tries to return a borrow of a local.
pub fn greeting(name: &str) -> String {
    String::new() // PLACEHOLDER — the real version needs a different signature, see the hint below
    // Hint: imagine `fn greeting(name: &str) -> &str { let text = format!("Hi {name}"); &text }`.
    // That's E0106/E0515: the String dies at the end of the function. Return the String itself.
}

/// Should add 1 to both counters. Two `&mut` to the same vector at once is E0499.
pub fn bump_both(counters: &mut Vec<u32>) {
    // PLACEHOLDER: the line below is the broken version; uncomment it to see E0499, then fix it
    // let a = &mut counters[0]; let b = &mut counters[1]; *a += 1; *b += 1;
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
