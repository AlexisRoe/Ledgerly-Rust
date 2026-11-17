//! The receipts module's outbox listeners. Java counterparts: `ScanReceipt.on(ReceiptUploaded)`, a tracked
//! `@Async @TransactionalEventListener`, and `ExpenseDeletedCleanup`, an `@ApplicationModuleListener`.
//!
//! Java's scan listens to a module-internal `ReceiptUploaded`; here it listens to the public
//! `ReceiptEvent` `Uploaded`, which carries the same ids and is published in the same transaction.

use ledgerly_expenses::{EXPENSE_DELETED, ExpenseEvent, ExpenseEventKind};
use ledgerly_shared::events::{Delivery, DeliveryFailed, Listener, read_payload};

use super::{ExpenseDeletedCleanup, ScanReceipt};
use crate::events::{RECEIPT_UPLOADED, ReceiptEvent, ReceiptEventKind};

/// Scans an uploaded receipt. A scan that cannot finish fails the delivery, so the outbox delivers the upload
/// again (Java's lesson from M9: a listener that fails must throw).
#[derive(Debug, Clone)]
pub(crate) struct ScanListener(pub ScanReceipt);

impl Listener for ScanListener {
    fn id(&self) -> &'static str {
        "ledgerly-rust:receipts.ScanReceipt"
    }

    fn listens_to(&self, event_type: &str) -> bool {
        event_type == RECEIPT_UPLOADED
    }

    fn deliver<'a>(&'a self, event_type: &'a str, payload: &'a str) -> Delivery<'a> {
        Box::pin(async move {
            let event: ReceiptEvent = read_payload(event_type, payload)?;
            if !matches!(event.kind, ReceiptEventKind::Uploaded { .. }) {
                return Ok(());
            }
            self.0
                .execute(event.team_id, event.receipt_id)
                .await
                .map_err(|failure| DeliveryFailed(format!("scan of receipt {}: {failure}", event.receipt_id)))
        })
    }
}

/// Removes the receipts of a deleted expense.
#[derive(Debug, Clone)]
pub(crate) struct CleanupListener(pub ExpenseDeletedCleanup);

impl Listener for CleanupListener {
    fn id(&self) -> &'static str {
        "ledgerly-rust:receipts.ExpenseDeletedCleanup"
    }

    fn listens_to(&self, event_type: &str) -> bool {
        event_type == EXPENSE_DELETED
    }

    fn deliver<'a>(&'a self, event_type: &'a str, payload: &'a str) -> Delivery<'a> {
        Box::pin(async move {
            let event: ExpenseEvent = read_payload(event_type, payload)?;
            if event.kind != ExpenseEventKind::Deleted {
                return Ok(());
            }
            self.0
                .execute(event.team_id, event.expense_id)
                .await
                .map_err(|_| DeliveryFailed(format!("receipts of deleted expense {}: database", event.expense_id)))
        })
    }
}
