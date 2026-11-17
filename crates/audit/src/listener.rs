//! One audit row per event. Java counterpart: `audit/internal/application/AuditListeners.java` (one
//! `@ApplicationModuleListener` per event type) and `AuditRecorder`.
//!
//! Here one outbox listener takes all 25 types, and one function per module maps its event to an [`Entry`]. Each
//! function matches every kind of its module's event, so a new kind does not compile until it says what to
//! record; a new event *type* must also be added to [`AUDITED_EVENT_TYPES`], which a test checks.

use std::sync::Arc;

use ledgerly_expenses::{ExpenseEvent, ExpenseEventKind};
use ledgerly_identity::events::IdentityEvent;
use ledgerly_imports::events::ImportCompleted;
use ledgerly_receipts::events::{ReceiptEvent, ReceiptEventKind};
use ledgerly_shared::clock::Clock;
use ledgerly_shared::events::{Delivery, DeliveryFailed, Event, Listener, read_payload};
use ledgerly_teams::events::{TeamEvent, TeamEventKind};
use serde_json::json;
use sqlx::PgPool;

use crate::trail::{self, Entry};

/// Every event type the trail records, as the outbox names them.
pub const AUDITED_EVENT_TYPES: [&str; 25] = [
    "ledgerly-rust:identity.UserRegistered",
    "ledgerly-rust:identity.UserVerified",
    "ledgerly-rust:identity.RefreshTokenReuseDetected",
    "ledgerly-rust:teams.TeamCreated",
    "ledgerly-rust:teams.MemberInvited",
    "ledgerly-rust:teams.InvitationRevoked",
    "ledgerly-rust:teams.InvitationAccepted",
    "ledgerly-rust:teams.InvitationDeclined",
    "ledgerly-rust:teams.MemberRoleChanged",
    "ledgerly-rust:teams.MemberRemoved",
    "ledgerly-rust:teams.MemberLeft",
    "ledgerly-rust:expenses.ExpenseCreated",
    "ledgerly-rust:expenses.ExpenseEdited",
    "ledgerly-rust:expenses.ExpenseDeleted",
    "ledgerly-rust:expenses.ExpenseSubmitted",
    "ledgerly-rust:expenses.ExpenseWithdrawn",
    "ledgerly-rust:expenses.ExpenseApproved",
    "ledgerly-rust:expenses.ExpenseRejected",
    "ledgerly-rust:expenses.ExpenseReopened",
    "ledgerly-rust:expenses.ExpenseReimbursed",
    "ledgerly-rust:receipts.ReceiptUploaded",
    "ledgerly-rust:receipts.ReceiptAccepted",
    "ledgerly-rust:receipts.ReceiptQuarantined",
    "ledgerly-rust:receipts.ReceiptDeleted",
    "ledgerly-rust:imports.ImportCompleted",
];

/// Records every audited event, in a transaction of its own after the publisher's committed.
#[derive(Debug, Clone)]
pub(crate) struct AuditListener {
    pub pool: PgPool,
    pub clock: Arc<dyn Clock>,
}

impl Listener for AuditListener {
    fn id(&self) -> &'static str {
        "ledgerly-rust:audit.AuditListeners"
    }

    fn listens_to(&self, event_type: &str) -> bool {
        AUDITED_EVENT_TYPES.contains(&event_type)
    }

    fn deliver<'a>(&'a self, event_type: &'a str, payload: &'a str) -> Delivery<'a> {
        Box::pin(async move {
            let entry = entry_of(event_type, payload)?;
            let mut connection = self
                .pool
                .acquire()
                .await
                .map_err(|_| DeliveryFailed("audit trail: database".to_owned()))?;
            trail::append(&mut connection, &entry, self.clock.now())
                .await
                .map_err(|_| DeliveryFailed("audit trail: database".to_owned()))
        })
    }
}

/// The row for one delivered event.
pub(crate) fn entry_of(event_type: &str, payload: &str) -> Result<Entry, DeliveryFailed> {
    let module = event_type
        .strip_prefix("ledgerly-rust:")
        .and_then(|rest| rest.split_once('.'))
        .map(|(module, _)| module);
    match module {
        Some("identity") => Ok(identity(&read_payload(event_type, payload)?)),
        Some("teams") => Ok(team(&read_payload(event_type, payload)?)),
        Some("expenses") => Ok(expense(&read_payload(event_type, payload)?)),
        Some("receipts") => Ok(receipt(&read_payload(event_type, payload)?)),
        Some("imports") => Ok(import(&read_payload(event_type, payload)?)),
        _ => Err(DeliveryFailed(format!("{event_type} is not audited"))),
    }
}

/// Account events have no team, and the user is both actor and subject.
fn identity(event: &IdentityEvent) -> Entry {
    let (event_type, details) = match event {
        IdentityEvent::UserRegistered { .. } => ("USER_REGISTERED", json!({})),
        IdentityEvent::UserVerified { .. } => ("USER_VERIFIED", json!({})),
        IdentityEvent::RefreshTokenReuseDetected { family_id, .. } => {
            ("REFRESH_TOKEN_REUSE_DETECTED", json!({ "familyId": family_id }))
        }
    };
    let user = event.actor_id().as_uuid();
    Entry {
        event_id: event.event_id(),
        team_id: None,
        actor_id: Some(user),
        event_type,
        subject_type: "USER",
        subject_id: user,
        details,
        occurred_at: event.occurred_at(),
    }
}

fn team(event: &TeamEvent) -> Entry {
    let (event_type, subject_type, subject_id, details) = match event.kind {
        TeamEventKind::TeamCreated => ("TEAM_CREATED", "TEAM", event.team_id.as_uuid(), json!({})),
        TeamEventKind::MemberInvited { invitation_id, role } => (
            "MEMBER_INVITED",
            "INVITATION",
            invitation_id.as_uuid(),
            json!({ "role": role }),
        ),
        TeamEventKind::InvitationRevoked { invitation_id } => {
            ("INVITATION_REVOKED", "INVITATION", invitation_id.as_uuid(), json!({}))
        }
        TeamEventKind::InvitationAccepted { invitation_id } => {
            ("INVITATION_ACCEPTED", "INVITATION", invitation_id.as_uuid(), json!({}))
        }
        TeamEventKind::InvitationDeclined { invitation_id } => {
            ("INVITATION_DECLINED", "INVITATION", invitation_id.as_uuid(), json!({}))
        }
        TeamEventKind::MemberRoleChanged {
            member_id,
            old_role,
            new_role,
        } => (
            "MEMBER_ROLE_CHANGED",
            "MEMBER",
            member_id.as_uuid(),
            json!({ "oldRole": old_role, "newRole": new_role }),
        ),
        TeamEventKind::MemberRemoved { member_id } => ("MEMBER_REMOVED", "MEMBER", member_id.as_uuid(), json!({})),
        TeamEventKind::MemberLeft => ("MEMBER_LEFT", "MEMBER", event.actor_id.as_uuid(), json!({})),
    };
    Entry {
        event_id: event.event_id,
        team_id: Some(event.team_id.as_uuid()),
        actor_id: Some(event.actor_id.as_uuid()),
        event_type,
        subject_type,
        subject_id,
        details,
        occurred_at: event.occurred_at,
    }
}

fn expense(event: &ExpenseEvent) -> Entry {
    let (event_type, details) = match &event.kind {
        ExpenseEventKind::Created => ("EXPENSE_CREATED", json!({})),
        ExpenseEventKind::Edited { changed_fields } => ("EXPENSE_EDITED", json!({ "changedFields": changed_fields })),
        ExpenseEventKind::Deleted => ("EXPENSE_DELETED", json!({})),
        ExpenseEventKind::Submitted => ("EXPENSE_SUBMITTED", json!({})),
        ExpenseEventKind::Withdrawn => ("EXPENSE_WITHDRAWN", json!({})),
        ExpenseEventKind::Approved => ("EXPENSE_APPROVED", json!({})),
        ExpenseEventKind::Rejected => ("EXPENSE_REJECTED", json!({})),
        ExpenseEventKind::Reopened => ("EXPENSE_REOPENED", json!({})),
        ExpenseEventKind::Reimbursed => ("EXPENSE_REIMBURSED", json!({})),
    };
    Entry {
        event_id: event.event_id,
        team_id: Some(event.team_id.as_uuid()),
        actor_id: Some(event.actor_id.as_uuid()),
        event_type,
        subject_type: "EXPENSE",
        subject_id: event.expense_id.as_uuid(),
        details,
        occurred_at: event.occurred_at,
    }
}

/// The scan's verdicts have no actor.
fn receipt(event: &ReceiptEvent) -> Entry {
    let event_type = match event.kind {
        ReceiptEventKind::Uploaded { .. } => "RECEIPT_UPLOADED",
        ReceiptEventKind::Accepted => "RECEIPT_ACCEPTED",
        ReceiptEventKind::Quarantined => "RECEIPT_QUARANTINED",
        ReceiptEventKind::Deleted { .. } => "RECEIPT_DELETED",
    };
    Entry {
        event_id: event.event_id,
        team_id: Some(event.team_id.as_uuid()),
        actor_id: event.actor_id().map(|actor| actor.as_uuid()),
        event_type,
        subject_type: "RECEIPT",
        subject_id: event.receipt_id.as_uuid(),
        details: json!({ "expenseId": event.expense_id }),
        occurred_at: event.occurred_at,
    }
}

fn import(event: &ImportCompleted) -> Entry {
    Entry {
        event_id: event.event_id,
        team_id: Some(event.team_id.as_uuid()),
        actor_id: Some(event.actor_id.as_uuid()),
        event_type: "IMPORT_COMPLETED",
        subject_type: "IMPORT",
        subject_id: event.import_id.as_uuid(),
        details: json!({
            "importedRows": event.imported_rows,
            "skippedRows": event.skipped_rows,
            "failed": event.failed,
        }),
        occurred_at: event.occurred_at,
    }
}

#[cfg(test)]
mod tests;
