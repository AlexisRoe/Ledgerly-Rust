//! Lesson 118: `macro_rules!` from scratch. Run `cargo test ex118`.
//!
//! Macros must be defined BEFORE they're used in a file. `cargo expand` (an optional tool) shows what they
//! expand to.

/// 1. `square!(x)` expands to `(x) * (x)`. One rule: `($value:expr) => { … }`. The parentheses in the expansion
///    matter: try `square!(1 + 2)` without them in your head.
macro_rules! square {
    ($value:expr) => {
        ($value) * ($value)
    };
}

/// 2. `string_map!{ "a" => 1, "b" => 2 }` builds a `HashMap<String, i32>`. Repetition: `$( … ),*` matches a
///    comma-separated list, and `$( … )*` in the expansion repeats once per match. Allow a trailing comma with
///    `$(,)?`.
macro_rules! string_map {
    ($($key:expr => $value:expr),* $(,)?) => {{
        let mut map = std::collections::HashMap::new();
        $( map.insert(String::from($key), $value); )*
        map
    }};
}

/// 3. Like Ledgerly's `display_as_text!`: for every listed type, generate `impl Display` writing `self.0`.
///    The pattern is `($($name:ident),+)`; the expansion wraps the whole impl in `$( … )+`.
macro_rules! display_inner {
    ($($name:ident),+) => {
        $(impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(&self.0)
            }
        })+
    };
}

pub struct Merchant(pub String);
pub struct Note(pub String);

display_inner!(Merchant, Note);

/// Uses of the macros, so the tests can call plain functions.
pub fn nine() -> i32 {
    square!(1 + 2)
}

pub fn prices() -> std::collections::HashMap<String, i32> {
    string_map! {
        "taxi" => 12,
        "hotel" => 90,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn squares_an_expression() {
        assert_eq!(nine(), 9);
    }

    #[test]
    fn builds_a_map() {
        let map = prices();
        assert_eq!(map.get("taxi"), Some(&12));
        assert_eq!(map.len(), 2);
    }

    // 4. After step 3, uncomment this test.
    #[test]
    fn displays_inner_text() {
        assert_eq!(Merchant("Cafe".to_owned()).to_string(), "Cafe");
        assert_eq!(Note("lunch".to_owned()).to_string(), "lunch");
    }
}
