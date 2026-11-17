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

    pub fn log_members(&mut self, prefix: &str) {
        let name = &self.name;
        for member in &self.members {
            if member.starts_with(prefix) {
                self.log.push(format!("{name}: {member}"));
            }
        }
    }
}

pub fn word_counts(text: &str) -> HashMap<String, usize> {
    let mut counts = HashMap::new();
    for word in text.split_whitespace() {
        *counts.entry(word.to_owned()).or_insert(0) += 1;
    }
    counts
}

pub fn list_for<'m>(lists: &'m mut HashMap<String, Vec<u32>>, key: &str) -> &'m mut Vec<u32> {
    lists.entry(key.to_owned()).or_default()
}

pub fn transfer(values: &mut [i64], from: usize, to: usize, amount: i64) -> bool {
    match values.get_disjoint_mut([from, to]) {
        Ok([source, target]) => {
            *source -= amount;
            *target += amount;
            true
        }
        Err(_) => false,
    }
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
