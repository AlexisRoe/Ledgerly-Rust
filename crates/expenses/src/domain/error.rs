use ledgerly_shared::role::Role;

use super::action::Action;
use super::status::ExpenseStatus;
use super::text::InvalidInput;

/// Everything the expense domain refuses to do. The messages are Java's.
///
/// Java counterpart: the sealed `ExpenseException` and its six subclasses in `expenses/internal/domain/`.
/// Java maps each subclass to an HTTP status in an exception handler; here a `match` over this enum will do
/// that (M3), and a new variant will not compile until it has a mapping.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ExpenseError {
    /// The action is not possible from the expense's current status, for example approving a draft.
    #[error("Expense is {from} and cannot {action}")]
    InvalidTransition { from: ExpenseStatus, action: Action },
    /// A claimant tried to approve, reject or reimburse their own expense.
    #[error("Claimants cannot {action} their own expense")]
    SelfDealing { action: Action },
    /// The actor's role, or their relation to the expense, does not permit the operation. Java: `NotAllowed`.
    #[error(transparent)]
    Forbidden(#[from] Forbidden),
    /// The state of the expense breaks a business rule, for example submitting without an accepted receipt.
    #[error("{0}")]
    RuleViolated(&'static str),
    /// A single field value breaks a rule.
    #[error(transparent)]
    InvalidInput(#[from] InvalidInput),
    /// Something only a draft allows (edit, delete, changing receipts) was attempted in another status.
    #[error("Only a draft allows this, but the expense is {status}")]
    NotDraft { status: ExpenseStatus },
}

/// Why an actor may not do something to an expense.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Forbidden {
    /// Submit, withdraw and reopen are the claimant's alone.
    #[error("Only the claimant may {0}")]
    NotClaimant(Action),
    /// Approve and reject need an approver, reimburse an owner.
    #[error("Role {role} may not {action}")]
    RoleTooLow { role: Role, action: Action },
    #[error("Only the claimant may edit an expense")]
    EditByNonClaimant,
    #[error("Only the claimant may change receipts")]
    ChangeReceiptsByNonClaimant,
    #[error("Only the claimant may delete an expense")]
    DeleteByNonClaimant,
}
