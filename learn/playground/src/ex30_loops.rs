//! Lesson 30: loops. Run `cargo test ex30`.

/// Sum of 1..=n using a `for` loop over a range (no formula).
pub fn sum_to(n: u32) -> u32 {
    todo!()
}

/// Simulates retrying: `attempts` lists whether each try succeeded. Return the 1-based number of the first
/// successful try, or None. Use `loop` with a counter and `break` with a value; stop when you run out.
/// Hint: `attempts.get(index)` gives `Option<&bool>`.
pub fn first_success(attempts: &[bool]) -> Option<usize> {
    let mut index = 0;
    let result = loop {
        todo!()
    };
    result
}

/// Counts the rows that have no empty cell. Use a labelled outer loop and `continue 'rows`.
pub fn complete_rows(rows: &[Vec<&str>]) -> usize {
    let mut complete = 0;
    todo!();
    complete
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sums() {
        assert_eq!(sum_to(4), 10);
        assert_eq!(sum_to(0), 0);
    }

    #[test]
    fn finds_first_success() {
        assert_eq!(first_success(&[false, false, true, true]), Some(3));
        assert_eq!(first_success(&[false]), None);
        assert_eq!(first_success(&[]), None);
    }

    #[test]
    fn counts_complete_rows() {
        let rows = vec![vec!["a", "b"], vec!["a", ""], vec!["c", "d"]];
        assert_eq!(complete_rows(&rows), 2);
    }
}
