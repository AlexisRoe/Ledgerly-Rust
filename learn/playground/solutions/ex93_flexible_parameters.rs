//! Lesson 93: `AsRef`, `Into` and flexible parameters. Run `cargo test ex93`.

/// 1. Accept anything that can be viewed as a `&str`: `&str`, `String`, `&String`, `Box<str>`…
///    Return "Hello, <name>!". Use `name.as_ref()`.
pub fn greet(name: impl AsRef<str>) -> String {
    format!("Hello, {}!", name.as_ref())
}

/// An error response. The constructors take `impl Into<String>`, so callers pass a literal or a String,
/// and the function does the one conversion it needs (no allocation if a String is passed in).
#[derive(Debug, PartialEq, Eq)]
pub struct Problem {
    pub title: String,
    pub detail: Option<String>,
}

impl Problem {
    /// 2. Store `title.into()`; no detail.
    pub fn new(title: impl Into<String>) -> Self {
        Self { title: title.into(), detail: None }
    }

    /// 3. Builder-style: take `self` by value, set the detail, return `self`.
    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }
}

/// 4. Total length in bytes of any list of string-like items: `Vec<String>`, `[&str; 3]`, `&[String]`…
///    Two type parameters: the collection `I`, and its item `S`.
pub fn total_len<I, S>(items: I) -> usize
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    items.into_iter().map(|item| item.as_ref().len()).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn greets_any_string_like_value() {
        let owned = String::from("Sam");
        assert_eq!(greet("Ana"), "Hello, Ana!");
        assert_eq!(greet(&owned), "Hello, Sam!");
        assert_eq!(greet(owned), "Hello, Sam!");
    }

    #[test]
    fn builds_problems() {
        let problem = Problem::new("Not found").with_detail(format!("expense {}", 7));
        assert_eq!(problem.title, "Not found");
        assert_eq!(problem.detail.as_deref(), Some("expense 7"));
        assert_eq!(Problem::new(String::from("x")).detail, None);
    }

    #[test]
    fn sums_lengths() {
        assert_eq!(total_len(["ab", "c"]), 3);
        assert_eq!(total_len(vec![String::from("abcd")]), 4);
        let names = [String::from("x"), String::from("yz")];
        assert_eq!(total_len(&names), 3);
    }
}
