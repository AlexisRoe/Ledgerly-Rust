//! The expenses module's events. Ids and field names only: no amounts, merchants, notes or rejection reasons,
//! which may be personal (Java's ADR 0001). The audit trail answers "who changed what", not "to what".
//!
//! Java counterpart: `expenses/events/ExpenseEvent.java` (the sealed `ExpenseEvent` and its nine records).

use chrono::{DateTime, Utc};
use ledgerly_shared::events::Event;
use ledgerly_shared::id::{ExpenseId, TeamId, UserId};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Everything that happens to an expense. Java: the sealed `ExpenseEvent`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExpenseEvent {
    pub event_id: Uuid,
    /// The team the expense belongs to.
    pub team_id: TeamId,
    /// The expense the event is about.
    pub expense_id: ExpenseId,
    /// Who did it.
    pub actor_id: UserId,
    /// When it happened, by the application clock.
    pub occurred_at: DateTime<Utc>,
    pub kind: ExpenseEventKind,
}

/// What happened. Java: one record per variant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExpenseEventKind {
    /// A draft was created: through the API, a batch or an import. Java: `Created`.
    Created,
    /// A draft was edited. The names of the fields that changed, in Java's order (`money`, `category`,
    /// `incurredOn`, `merchant`, `note`), never their values. Java: `Edited`.
    Edited { changed_fields: Vec<String> },
    /// A draft was deleted; receipts removes its files in response. Java: `Deleted`.
    Deleted,
    /// A draft was submitted for approval. Java: `Submitted`.
    Submitted,
    /// The claimant took a submitted expense back to draft. Java: `Withdrawn`.
    Withdrawn,
    /// An approver approved a submitted expense. Java: `Approved`.
    Approved,
    /// The reason stays with the expense, not in the event. Java: `Rejected`.
    Rejected,
    /// The claimant moved a rejected expense back to draft. Java: `Reopened`.
    Reopened,
    /// An owner marked an approved expense as paid out. Java: `Reimbursed`.
    Reimbursed,
}

/// The `event_type` of [`ExpenseEventKind::Deleted`], which receipts listens to.
pub const EXPENSE_DELETED: &str = "ledgerly-rust:expenses.ExpenseDeleted";

impl ExpenseEvent {
    pub(crate) fn new(
        team_id: TeamId,
        expense_id: ExpenseId,
        actor_id: UserId,
        occurred_at: DateTime<Utc>,
        kind: ExpenseEventKind,
    ) -> Self {
        Self {
            event_id: Uuid::now_v7(),
            team_id,
            expense_id,
            actor_id,
            occurred_at,
            kind,
        }
    }
}

impl Event for ExpenseEvent {
    fn event_id(&self) -> Uuid {
        self.event_id
    }

    fn occurred_at(&self) -> DateTime<Utc> {
        self.occurred_at
    }

    fn event_type(&self) -> &'static str {
        match self.kind {
            ExpenseEventKind::Created => "ledgerly-rust:expenses.ExpenseCreated",
            ExpenseEventKind::Edited { .. } => "ledgerly-rust:expenses.ExpenseEdited",
            ExpenseEventKind::Deleted => EXPENSE_DELETED,
            ExpenseEventKind::Submitted => "ledgerly-rust:expenses.ExpenseSubmitted",
            ExpenseEventKind::Withdrawn => "ledgerly-rust:expenses.ExpenseWithdrawn",
            ExpenseEventKind::Approved => "ledgerly-rust:expenses.ExpenseApproved",
            ExpenseEventKind::Rejected => "ledgerly-rust:expenses.ExpenseRejected",
            ExpenseEventKind::Reopened => "ledgerly-rust:expenses.ExpenseReopened",
            ExpenseEventKind::Reimbursed => "ledgerly-rust:expenses.ExpenseReimbursed",
        }
    }

    fn payload(&self) -> serde_json::Result<String> {
        serde_json::to_string(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_kind_has_its_own_type_and_the_payload_reads_back() {
        let kinds = [
            ExpenseEventKind::Created,
            ExpenseEventKind::Edited {
                changed_fields: vec!["merchant".to_owned(), "note".to_owned()],
            },
            ExpenseEventKind::Deleted,
            ExpenseEventKind::Submitted,
            ExpenseEventKind::Withdrawn,
            ExpenseEventKind::Approved,
            ExpenseEventKind::Rejected,
            ExpenseEventKind::Reopened,
            ExpenseEventKind::Reimbursed,
        ];
        let mut types = std::collections::BTreeSet::new();
        for kind in kinds {
            let event = ExpenseEvent::new(
                TeamId::new_v7(),
                ExpenseId::new_v7(),
                UserId::new_v7(),
                Utc::now(),
                kind,
            );
            assert!(event.event_type().starts_with("ledgerly-rust:expenses.Expense"));
            types.insert(event.event_type());
            let back: ExpenseEvent = serde_json::from_str(&event.payload().unwrap()).unwrap();
            assert_eq!(back, event);
        }
        assert_eq!(types.len(), 9);
    }
}
