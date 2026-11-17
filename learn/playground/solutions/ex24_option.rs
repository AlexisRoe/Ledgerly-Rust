//! Lesson 24: Option. Run `cargo test ex24`.
//! Each function has the TypeScript it translates. Use Option's methods, not `match` or `unwrap()`.

pub struct User {
    pub name: String,
    pub nickname: Option<String>,
    pub manager_id: Option<u32>,
}

/// TS: `return user.nickname ?? user.name;`
/// Hint: `user.nickname.clone().unwrap_or(...)`; `.clone()` copies the String out of the borrowed user.
pub fn display_name(user: &User) -> String {
    user.nickname.clone().unwrap_or(user.name.clone())
}

/// TS: `return user.nickname?.length;`   (undefined when there's no nickname)
/// Hint: `.as_ref()` turns `&Option<String>` into `Option<&String>`, then `.map(...)`.
pub fn nickname_length(user: &User) -> Option<usize> {
    user.nickname.as_ref().map(|n| n.len())
}

/// TS: `return amount !== undefined && amount > 0 ? amount : undefined;`
pub fn positive(amount: Option<i64>) -> Option<i64> {
    amount.filter(|a| *a > 0)
}

/// TS: `if (user.managerId === undefined) throw new Error("no manager"); return user.managerId;`
/// In Rust: return a Result. Hint: `ok_or`.
pub fn manager(user: &User) -> Result<u32, &'static str> {
    user.manager_id.ok_or("no manager")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alexis() -> User {
        User { name: "Alexis".to_owned(), nickname: Some("Lex".to_owned()), manager_id: None }
    }

    fn sam() -> User {
        User { name: "Sam".to_owned(), nickname: None, manager_id: Some(7) }
    }

    #[test]
    fn display_name_falls_back() {
        assert_eq!(display_name(&alexis()), "Lex");
        assert_eq!(display_name(&sam()), "Sam");
    }

    #[test]
    fn nickname_length_maps() {
        assert_eq!(nickname_length(&alexis()), Some(3));
        assert_eq!(nickname_length(&sam()), None);
    }

    #[test]
    fn positive_filters() {
        assert_eq!(positive(Some(5)), Some(5));
        assert_eq!(positive(Some(-5)), None);
        assert_eq!(positive(None), None);
    }

    #[test]
    fn manager_becomes_result() {
        assert_eq!(manager(&sam()), Ok(7));
        assert_eq!(manager(&alexis()), Err("no manager"));
    }
}
