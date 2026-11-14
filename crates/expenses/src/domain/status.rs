use std::fmt;
use std::str::FromStr;

/// Where an expense stands in the workflow.
///
/// Draft → Submitted → Approved → Reimbursed, or Submitted → Rejected → Draft (reopen).
///
/// Java counterpart: `expenses/internal/domain/ExpenseStatus.java`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExpenseStatus {
    Draft,
    Submitted,
    Approved,
    Rejected,
    Reimbursed,
}

/// The text is not the name of an [`ExpenseStatus`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("Unknown expense status: {0}")]
pub struct UnknownExpenseStatus(String);

impl ExpenseStatus {
    /// Every status, in the order Java declares them.
    pub const ALL: [Self; 5] = [
        Self::Draft,
        Self::Submitted,
        Self::Approved,
        Self::Rejected,
        Self::Reimbursed,
    ];

    /// The name as Java, the database and the API write it, such as `DRAFT`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Draft => "DRAFT",
            Self::Submitted => "SUBMITTED",
            Self::Approved => "APPROVED",
            Self::Rejected => "REJECTED",
            Self::Reimbursed => "REIMBURSED",
        }
    }
}

impl fmt::Display for ExpenseStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for ExpenseStatus {
    type Err = UnknownExpenseStatus;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|status| status.as_str() == s)
            .ok_or_else(|| UnknownExpenseStatus(s.to_owned()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_javas_enum_constants_in_javas_order() {
        let names: Vec<&str> = ExpenseStatus::ALL.iter().map(|status| status.as_str()).collect();
        assert_eq!(names, ["DRAFT", "SUBMITTED", "APPROVED", "REJECTED", "REIMBURSED"]);
    }

    #[test]
    fn every_status_parses_from_its_name_and_prints_it() {
        for status in ExpenseStatus::ALL {
            assert_eq!(status.as_str().parse::<ExpenseStatus>(), Ok(status));
            assert_eq!(status.to_string(), status.as_str());
        }
    }

    #[test]
    fn other_text_is_not_a_status() {
        for text in ["", "draft", "Draft", "PAID", "CANCELLED"] {
            let error = text.parse::<ExpenseStatus>().unwrap_err();
            assert_eq!(error.to_string(), format!("Unknown expense status: {text}"));
        }
    }
}
