use std::fmt;
use std::str::FromStr;

/// What an expense was spent on: one of a fixed, system-wide set.
///
/// Java counterpart: `expenses/Category.java`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Category {
    Travel,
    Meals,
    Office,
    Software,
    Other,
}

/// The text is not the name of a [`Category`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("Unknown category: {0}")]
pub struct UnknownCategory(String);

impl Category {
    /// Every category, in the order Java declares them.
    pub const ALL: [Self; 5] = [Self::Travel, Self::Meals, Self::Office, Self::Software, Self::Other];

    /// The name as Java, the database and the API write it, such as `TRAVEL`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Travel => "TRAVEL",
            Self::Meals => "MEALS",
            Self::Office => "OFFICE",
            Self::Software => "SOFTWARE",
            Self::Other => "OTHER",
        }
    }
}

impl fmt::Display for Category {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Category {
    type Err = UnknownCategory;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|category| category.as_str() == s)
            .ok_or_else(|| UnknownCategory(s.to_owned()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_javas_enum_constants_in_javas_order() {
        let names: Vec<&str> = Category::ALL.iter().map(|category| category.as_str()).collect();
        assert_eq!(names, ["TRAVEL", "MEALS", "OFFICE", "SOFTWARE", "OTHER"]);
    }

    #[test]
    fn every_category_parses_from_its_name_and_prints_it() {
        for category in Category::ALL {
            assert_eq!(category.as_str().parse::<Category>(), Ok(category));
            assert_eq!(category.to_string(), category.as_str());
        }
    }

    #[test]
    fn other_text_is_not_a_category() {
        for text in ["", "travel", "Meals", "FOOD", " OTHER"] {
            let error = text.parse::<Category>().unwrap_err();
            assert_eq!(error.to_string(), format!("Unknown category: {text}"));
        }
    }
}
