//! Lesson 81: structs that borrow. Run `cargo test ex81`.

/// A parsed `Name: value` header line. It borrows both parts from the line instead of copying them.
#[derive(Debug, PartialEq, Eq)]
pub struct Header<'a> {
    pub name: &'a str,
    pub value: &'a str,
}

/// 1. Split `line` at its first `:`; trim the value. `None` without a colon. `Header<'_>` means "borrows from
///    the one input reference" (elision rule 2). Hint: `split_once`.
pub fn parse_header(line: &str) -> Option<Header<'_>> {
    todo!()
}

/// A cursor over the words of a text. It holds a `&'a str` and moves through it without allocating.
pub struct Words<'a> {
    rest: &'a str,
}

impl<'a> Words<'a> {
    pub fn new(text: &'a str) -> Self {
        Self { rest: text }
    }

    /// 2. The next word (split on whitespace), or `None` at the end. Note the return type: `&'a str`, tied to
    ///    the *text*, not to `&mut self`. So a word stays usable after the next call. (Lesson 81 shows what
    ///    goes wrong with a plain `&str` here.)
    ///    Hint: trim the start of `rest`, find the next whitespace with `find(char::is_whitespace)`, and keep
    ///    what comes after it in `self.rest`.
    pub fn next_word(&mut self) -> Option<&'a str> {
        todo!()
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
