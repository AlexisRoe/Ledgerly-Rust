use std::fmt;

use super::status::ExpenseStatus;

/// The six workflow transitions.
///
/// Java counterpart: `expenses/internal/domain/Action.java`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Action {
    Submit,
    Withdraw,
    Approve,
    Reject,
    Reopen,
    Reimburse,
}

impl Action {
    /// Every action, in the order Java declares them.
    pub const ALL: [Self; 6] = [
        Self::Submit,
        Self::Withdraw,
        Self::Approve,
        Self::Reject,
        Self::Reopen,
        Self::Reimburse,
    ];

    /// The name as Java writes it in messages, such as `SUBMIT`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Submit => "SUBMIT",
            Self::Withdraw => "WITHDRAW",
            Self::Approve => "APPROVE",
            Self::Reject => "REJECT",
            Self::Reopen => "REOPEN",
            Self::Reimburse => "REIMBURSE",
        }
    }

    /// The whole workflow: where this action leads from the status `from`, or `None` if it is not possible there.
    ///
    /// ```text
    /// Draft ──Submit──► Submitted ──Approve──► Approved ──Reimburse──► Reimbursed
    ///   ▲ ◄──Withdraw──────┘ │
    ///   │                  Reject
    ///   └────Reopen──── Rejected ◄┘
    /// ```
    ///
    /// Both matches are exhaustive on purpose, without a `_` arm: a new status or a new action does not compile
    /// until each of its pairs is decided here. Java spreads the same table over six methods, each with a
    /// `requireStatus` call, and relies on its 30-case test to notice a forgotten pair.
    #[must_use]
    pub const fn target_from(self, from: ExpenseStatus) -> Option<ExpenseStatus> {
        match from {
            ExpenseStatus::Draft => match self {
                Self::Submit => Some(ExpenseStatus::Submitted),
                Self::Withdraw | Self::Approve | Self::Reject | Self::Reopen | Self::Reimburse => None,
            },
            ExpenseStatus::Submitted => match self {
                Self::Withdraw => Some(ExpenseStatus::Draft),
                Self::Approve => Some(ExpenseStatus::Approved),
                Self::Reject => Some(ExpenseStatus::Rejected),
                Self::Submit | Self::Reopen | Self::Reimburse => None,
            },
            ExpenseStatus::Approved => match self {
                Self::Reimburse => Some(ExpenseStatus::Reimbursed),
                Self::Submit | Self::Withdraw | Self::Approve | Self::Reject | Self::Reopen => None,
            },
            ExpenseStatus::Rejected => match self {
                Self::Reopen => Some(ExpenseStatus::Draft),
                Self::Submit | Self::Withdraw | Self::Approve | Self::Reject | Self::Reimburse => None,
            },
            ExpenseStatus::Reimbursed => match self {
                Self::Submit | Self::Withdraw | Self::Approve | Self::Reject | Self::Reopen | Self::Reimburse => None,
            },
        }
    }
}

impl fmt::Display for Action {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_javas_enum_constants_in_javas_order() {
        let names: Vec<String> = Action::ALL.iter().map(ToString::to_string).collect();
        assert_eq!(
            names,
            ["SUBMIT", "WITHDRAW", "APPROVE", "REJECT", "REOPEN", "REIMBURSE"]
        );
    }

    #[test]
    fn the_workflow_has_exactly_six_edges() {
        let mut edges = Vec::new();
        for from in ExpenseStatus::ALL {
            for action in Action::ALL {
                if let Some(to) = action.target_from(from) {
                    edges.push(format!("{from} -{action}-> {to}"));
                }
            }
        }
        assert_eq!(
            edges,
            [
                "DRAFT -SUBMIT-> SUBMITTED",
                "SUBMITTED -WITHDRAW-> DRAFT",
                "SUBMITTED -APPROVE-> APPROVED",
                "SUBMITTED -REJECT-> REJECTED",
                "APPROVED -REIMBURSE-> REIMBURSED",
                "REJECTED -REOPEN-> DRAFT",
            ]
        );
    }

    #[test]
    fn reimbursed_is_final() {
        assert!(
            Action::ALL
                .iter()
                .all(|action| action.target_from(ExpenseStatus::Reimbursed).is_none())
        );
    }
}
