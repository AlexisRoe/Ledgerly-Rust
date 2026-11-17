//! Lesson 83: borrowing in practice. Run `cargo test ex83`.

use std::collections::HashMap;

pub struct Team {
    pub name: String,
    pub members: Vec<String>,
    pub log: Vec<String>,
}

impl Team {
    fn label(&self, member: &str) -> String {
        format!("{}: {member}", self.name)
    }

    /// 1. For every member starting with `prefix`, push "<team name>: <member>" to `self.log`.
    ///    First try calling `self.label(member)` inside a `for member in &self.members` loop and read the
    ///    error (E0502). Then fix it by borrowing the *fields* separately: `let name = &self.name;` and use
    ///    `name` instead of the method. The compiler tracks fields one by one, but a method call borrows all
    ///    of `self`.
    pub fn log_members(&mut self, prefix: &str) {
        todo!()
    }
}

/// 2. How often each word occurs. Use the entry API: `*counts.entry(word.to_owned()).or_insert(0) += 1;`
///    instead of a `get` followed by an `insert` (TS: `map.set(w, (map.get(w) ?? 0) + 1)`).
pub fn word_counts(text: &str) -> HashMap<String, usize> {
    todo!()
}

/// 3. The list stored under `key`, created empty if missing, ready to be changed by the caller.
///    One `entry(…).or_default()` does both. (A `get_mut`, then an `insert`, then another `get_mut` also
///    works but does three lookups.)
pub fn list_for<'m>(lists: &'m mut HashMap<String, Vec<u32>>, key: &str) -> &'m mut Vec<u32> {
    todo!()
}

/// 4. Move `amount` from `values[from]` to `values[to]`. Two `&mut` into one slice at the same time needs
///    `values.get_disjoint_mut([from, to])`, which checks that the indexes differ and are in bounds.
///    Return false (and change nothing) if that check fails.
pub fn transfer(values: &mut [i64], from: usize, to: usize, amount: i64) -> bool {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logs_members() {
        let mut team = Team {
            name: "Ops".to_owned(),
            members: vec!["sam".to_owned(), "alex".to_owned(), "sasha".to_owned()],
            log: Vec::new(),
        };
        team.log_members("sa");
        assert_eq!(team.log, ["Ops: sam", "Ops: sasha"]);
    }

    #[test]
    fn counts_words() {
        let counts = word_counts("taxi hotel taxi taxi");
        assert_eq!(counts.get("taxi"), Some(&3));
        assert_eq!(counts.get("hotel"), Some(&1));
        assert_eq!(counts.len(), 2);
    }

    #[test]
    fn creates_lists_on_demand() {
        let mut lists = HashMap::new();
        list_for(&mut lists, "a").push(1);
        list_for(&mut lists, "a").push(2);
        list_for(&mut lists, "b");
        assert_eq!(lists["a"], [1, 2]);
        assert!(lists["b"].is_empty());
    }

    #[test]
    fn transfers_between_two_slots() {
        let mut values = [10, 0, 5];
        assert!(transfer(&mut values, 0, 2, 4));
        assert_eq!(values, [6, 0, 9]);
        assert!(!transfer(&mut values, 1, 1, 4));
        assert!(!transfer(&mut values, 0, 9, 4));
        assert_eq!(values, [6, 0, 9]);
    }
}
