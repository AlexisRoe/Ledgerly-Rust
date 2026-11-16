//! The receipts module's events. Ids only, never a file name or content (Java's ADR 0001).
//!
//! Java counterpart: `receipts/events/ReceiptEvent.java` (the sealed `ReceiptEvent` and its four records).
//! Java's module-internal `ReceiptUploaded`, which starts the scan, has no counterpart here: the scan listens to
//! the public `Uploaded`, which carries the same ids.

use chrono::{DateTime, Utc};
use ledgerly_shared::events::Event;
use ledgerly_shared::id::{ExpenseId, ReceiptId, TeamId, UserId};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Something that happened to a receipt. Java: the sealed `ReceiptEvent`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReceiptEvent {
    pub event_id: Uuid,
    /// The team the receipt's expense belongs to.
    pub team_id: TeamId,
    /// The expense the receipt is attached to.
    pub expense_id: ExpenseId,
    pub receipt_id: ReceiptId,
    /// When it happened, by the application clock.
    pub occurred_at: DateTime<Utc>,
    pub kind: ReceiptEventKind,
}

/// What happened. Java: one record per variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReceiptEventKind {
    /// A file was stored and waits for the malware scan. Java: `Uploaded`.
    Uploaded { actor_id: UserId },
    /// The scan found nothing; done by the system, so there is no actor. Java: `Accepted`.
    Accepted,
    /// The scan found malware; done by the system. Java: `Quarantined`.
    Quarantined,
    /// A user removed the receipt. Java: `Deleted`.
    Deleted { actor_id: UserId },
}

/// The `event_type` of [`ReceiptEventKind::Uploaded`], which starts the scan.
pub const RECEIPT_UPLOADED: &str = "ledgerly-rust:receipts.ReceiptUploaded";

impl ReceiptEvent {
    pub(crate) fn new(
        team_id: TeamId,
        expense_id: ExpenseId,
        receipt_id: ReceiptId,
        occurred_at: DateTime<Utc>,
        kind: ReceiptEventKind,
    ) -> Self {
        Self {
            event_id: Uuid::now_v7(),
            team_id,
            expense_id,
            receipt_id,
            occurred_at,
            kind,
        }
    }

    /// The user who did it; `None` when the system did (the malware scan).
    #[must_use]
    pub const fn actor_id(&self) -> Option<UserId> {
        match self.kind {
            ReceiptEventKind::Uploaded { actor_id } | ReceiptEventKind::Deleted { actor_id } => Some(actor_id),
            ReceiptEventKind::Accepted | ReceiptEventKind::Quarantined => None,
        }
    }
}

impl Event for ReceiptEvent {
    fn event_id(&self) -> Uuid {
        self.event_id
    }

    fn occurred_at(&self) -> DateTime<Utc> {
        self.occurred_at
    }

    fn event_type(&self) -> &'static str {
        match self.kind {
            ReceiptEventKind::Uploaded { .. } => RECEIPT_UPLOADED,
            ReceiptEventKind::Accepted => "ledgerly-rust:receipts.ReceiptAccepted",
            ReceiptEventKind::Quarantined => "ledgerly-rust:receipts.ReceiptQuarantined",
            ReceiptEventKind::Deleted { .. } => "ledgerly-rust:receipts.ReceiptDeleted",
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
    fn only_the_scan_has_no_actor_and_every_kind_has_its_own_type() {
        let (team, expense, receipt, user) = (
            TeamId::new_v7(),
            ExpenseId::new_v7(),
            ReceiptId::new_v7(),
            UserId::new_v7(),
        );
        let kinds = [
            (ReceiptEventKind::Uploaded { actor_id: user }, Some(user)),
            (ReceiptEventKind::Accepted, None),
            (ReceiptEventKind::Quarantined, None),
            (ReceiptEventKind::Deleted { actor_id: user }, Some(user)),
        ];
        let mut types = Vec::new();
        for (kind, actor) in kinds {
            let event = ReceiptEvent::new(team, expense, receipt, Utc::now(), kind);
            assert_eq!(event.actor_id(), actor);
            assert!(event.event_type().starts_with("ledgerly-rust:receipts."));
            types.push(event.event_type());
        }
        types.dedup();
        assert_eq!(types.len(), 4);
    }
}
