//! Lesson 80: writing lifetime annotations. Run `cargo test ex80`.

/// 1. The longer of two texts; the first one on a tie.
pub fn longer<'a>(a: &'a str, b: &'a str) -> &'a str {
    if b.len() > a.len() { b } else { a }
}

/// 2. The rest of `line` after `prefix`, or `None`.
pub fn after<'a>(line: &'a str, prefix: &str) -> Option<&'a str> {
    line.strip_prefix(prefix)
}

/// 3. The result borrows from `items` only, so `name` may be dropped first.
pub fn find<'a>(items: &'a [String], name: &str) -> Option<&'a String> {
    items.iter().find(|item| item.as_str() == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_the_longer_one() {
        let a = String::from("receipt");
        let b = String::from("note");
        assert_eq!(longer(&a, &b), "receipt");
        assert_eq!(longer(&b, &a), "receipt");
        assert_eq!(longer("ab", "cd"), "ab");
        assert_eq!(longer(&a, &b).as_ptr(), a.as_ptr());
    }

    #[test]
    fn strips_the_prefix() {
        let line = String::from("Bearer abc.def");
        let token = {
            let prefix = String::from("Bearer ");
            after(&line, &prefix)
        };
        assert_eq!(token, Some("abc.def"));
        assert_eq!(after("Basic x", "Bearer "), None);
    }

    #[test]
    fn found_outlives_the_name() {
        let items = vec![String::from("draft"), String::from("submitted")];
        let found = {
            let name = String::from("submitted");
            find(&items, &name)
        };
        assert_eq!(found.map(String::as_str), Some("submitted"));
    }
}
