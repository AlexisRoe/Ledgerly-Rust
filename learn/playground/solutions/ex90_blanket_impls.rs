//! Lesson 90: default methods, blanket impls and extension traits. Run `cargo test ex90`.

use std::fmt::Display;

/// 1. A trait with one required and one default method. Write the default body ONCE here:
///    "<title> (<n> chars)", where n counts the title's chars. Implementors get it for free.
pub trait Titled {
    fn title(&self) -> String;

    fn headline(&self) -> String {
        let title = self.title();
        let count = title.chars().count();
        format!("{title} ({count} chars)")
    }
}

pub struct Report;

impl Titled for Report {
    fn title(&self) -> String {
        "March".to_owned()
    }
}

/// 2. An extension trait: new methods on a type you don't own (`str`). Implement `initials`: the first char
///    of each whitespace-separated word, uppercased. "sam de vries" → "SDV".
pub trait StrExt {
    fn initials(&self) -> String;
}

impl StrExt for str {
    fn initials(&self) -> String {
        self.split_whitespace()
            .filter_map(|word| word.chars().next())
            .flat_map(char::to_uppercase)
            .collect()
    }
}

/// 3. A blanket impl: EVERY type that implements `Display` gets `shout`. Return the displayed text,
///    uppercased, with "!" appended. (`?Sized` lets it apply to `str` too.)
pub trait Shout {
    fn shout(&self) -> String;
}

impl<T: Display + ?Sized> Shout for T {
    fn shout(&self) -> String {
        format!("{}!", self.to_string().to_uppercase())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_method() {
        assert_eq!(Report.headline(), "March (5 chars)");
    }

    #[test]
    fn extension_trait() {
        assert_eq!("sam de vries".initials(), "SDV");
        assert_eq!(String::from("ana").initials(), "A");
    }

    #[test]
    fn blanket_impl() {
        assert_eq!(42.shout(), "42!");
        assert_eq!("hi".shout(), "HI!");
        assert_eq!(std::net::Ipv4Addr::LOCALHOST.shout(), "127.0.0.1!");
    }
}
