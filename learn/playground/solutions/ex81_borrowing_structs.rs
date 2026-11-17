//! Lesson 81: structs that borrow. Run `cargo test ex81`.

#[derive(Debug, PartialEq, Eq)]
pub struct Header<'a> {
    pub name: &'a str,
    pub value: &'a str,
}

pub fn parse_header(line: &str) -> Option<Header<'_>> {
    let (name, value) = line.split_once(':')?;
    Some(Header { name, value: value.trim() })
}

pub struct Words<'a> {
    rest: &'a str,
}

impl<'a> Words<'a> {
    pub fn new(text: &'a str) -> Self {
        Self { rest: text }
    }

    pub fn next_word(&mut self) -> Option<&'a str> {
        let text = self.rest.trim_start();
        if text.is_empty() {
            self.rest = text;
            return None;
        }
        let end = text.find(char::is_whitespace).unwrap_or(text.len());
        let (word, rest) = text.split_at(end);
        self.rest = rest;
        Some(word)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_header() {
        assert_eq!(
            parse_header("Content-Type:  text/csv "),
            Some(Header { name: "Content-Type", value: "text/csv" })
        );
        assert_eq!(parse_header("no colon"), None);
    }

    #[test]
    fn words_outlive_the_next_call() {
        let text = String::from("  submit   the expense ");
        let mut words = Words::new(&text);
        let first = words.next_word();
        let second = words.next_word();
        let third = words.next_word();
        assert_eq!((first, second, third), (Some("submit"), Some("the"), Some("expense")));
        assert_eq!(words.next_word(), None);
        assert_eq!(words.next_word(), None);
    }
}
