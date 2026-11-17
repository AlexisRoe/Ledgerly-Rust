//! Lesson 88: associated types vs generic parameters. Run `cargo test ex88`.

/// A source of records. Each implementing type produces ONE kind of record, so the record type is an
/// associated type: callers write `S::Item`, and never have to name it.
pub trait Source {
    type Item;
    fn next_record(&mut self) -> Option<Self::Item>;
}

/// Counts down from its number to 1.
pub struct Countdown(pub u32);

impl Source for Countdown {
    type Item = u32;

    /// 1. Yield the current number and decrease it; `None` once it is 0.
    fn next_record(&mut self) -> Option<u32> {
        if self.0 == 0 {
            return None;
        }
        self.0 -= 1;
        Some(self.0 + 1)
    }
}

/// Hands out names from the front of a list.
pub struct Names(pub Vec<String>);

impl Source for Names {
    /// 2. Implement `next_record` (hint: `if self.0.is_empty()`,
    ///    then `self.0.remove(0)`).
    type Item = String;

    fn next_record(&mut self) -> Option<String> {
        if self.0.is_empty() { None } else { Some(self.0.remove(0)) }
    }
}

/// 3. Works with ANY source. Note the return type: `Vec<S::Item>`, no second type parameter needed.
pub fn drain_all<S: Source>(source: &mut S) -> Vec<S::Item> {
    let mut all = Vec::new();
    while let Some(record) = source.next_record() {
        all.push(record);
    }
    all
}

/// A generic PARAMETER instead: one type may implement `Describe<English>` and `Describe<German>`, so the
/// caller picks which one. That's the difference: a parameter allows many impls per type, an associated type one.
pub trait Describe<Lang> {
    fn describe(&self) -> String;
}

pub struct English;
pub struct German;
pub struct Draft;

impl Describe<English> for Draft {
    fn describe(&self) -> String {
        "draft".to_owned()
    }
}

// 4. Write `impl Describe<German> for Draft` returning "Entwurf", then uncomment the test `speaks_german`.
impl Describe<German> for Draft {
    fn describe(&self) -> String {
        "Entwurf".to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_down() {
        assert_eq!(drain_all(&mut Countdown(3)), [3, 2, 1]);
    }

    #[test]
    fn hands_out_names() {
        let mut names = Names(vec!["sam".to_owned(), "alex".to_owned()]);
        assert_eq!(names.next_record().as_deref(), Some("sam"));
        assert_eq!(drain_all(&mut names), ["alex"]);
    }

    #[test]
    fn speaks_german() {
        assert_eq!(<Draft as Describe<English>>::describe(&Draft), "draft");
        assert_eq!(<Draft as Describe<German>>::describe(&Draft), "Entwurf");
    }
}
