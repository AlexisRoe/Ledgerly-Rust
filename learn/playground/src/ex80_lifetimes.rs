//! Lesson 80: writing lifetime annotations. Run `cargo test ex80`.

/// 1. The longer of two texts; the first one on a tie. The signature is done: `'a` says "the result borrows
///    from `a` or `b`, so it's valid only while both are".
pub fn longer<'a>(a: &'a str, b: &'a str) -> &'a str {
    todo!()
}

/// 2. The rest of `line` after `prefix`, or `None` if it doesn't start with it. The result borrows only from
///    `line`, so only `line` shares the output's lifetime; `prefix` gets its own (elided) one.
///    Hint: `str::strip_prefix`.
pub fn after<'a>(line: &'a str, prefix: &str) -> Option<&'a str> {
    todo!()
}

// 3. Uncomment `find` and the test `found_outlives_the_name` below. It doesn't compile: elision can't choose
//    between two input references. Add one lifetime so the result borrows from `items` only.
//
// pub fn find(items: &[String], name: &str) -> Option<&String> {
//     items.iter().find(|item| item.as_str() == name)
// }

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
        // Borrowed, not copied: the result points into `a`.
        assert_eq!(longer(&a, &b).as_ptr(), a.as_ptr());
    }

    #[test]
    fn strips_the_prefix() {
        let line = String::from("Bearer abc.def");
        let token = {
            let prefix = String::from("Bearer ");
            after(&line, &prefix)
        };
        // `prefix` is gone here, and `token` is still fine: it borrows only from `line`.
        assert_eq!(token, Some("abc.def"));
        assert_eq!(after("Basic x", "Bearer "), None);
    }

    // #[test]
    // fn found_outlives_the_name() {
    //     let items = vec![String::from("draft"), String::from("submitted")];
    //     let found = {
    //         let name = String::from("submitted");
    //         find(&items, &name)
    //     };
    //     assert_eq!(found.map(String::as_str), Some("submitted"));
    // }
}
