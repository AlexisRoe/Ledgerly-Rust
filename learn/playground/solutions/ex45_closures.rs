//! Lesson 45: closures. Run `cargo test ex45`.

/// Apply a 19 % VAT to every amount. Store the rule in a closure `with_vat` first, then use it in the loop.
pub fn gross_all(nets: &[u32]) -> Vec<u32> {
    let with_vat = |net: u32| net * 119 / 100;
    let mut result = Vec::new();
    for net in nets {
        result.push(with_vat(*net));
    }
    result
}

/// Return the nickname, or build "user-<id>" lazily with `unwrap_or_else` and a closure.
pub fn handle(nickname: Option<String>, id: u32) -> String {
    nickname.unwrap_or_else(|| format!("user-{id}"))
}

/// Return a closure that adds `prefix` in front of any text. It must own the prefix: use `move`.
/// The return type `impl Fn(&str) -> String` means "some closure you can call with a &str".
pub fn prefixer(prefix: String) -> impl Fn(&str) -> String {
    move |text| format!("{prefix}{text}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gross() {
        assert_eq!(gross_all(&[100, 1000]), vec![119, 1190]);
    }

    #[test]
    fn handles() {
        assert_eq!(handle(Some("lex".to_owned()), 7), "lex");
        assert_eq!(handle(None, 7), "user-7");
    }

    #[test]
    fn prefixes() {
        let p = prefixer("EUR ".to_owned());
        assert_eq!(p("12.50"), "EUR 12.50");
        assert_eq!(p("3.00"), "EUR 3.00");
    }
}
