//! The JSON bodies of the receipts endpoints, as `api/openapi.yaml` describes them.
//!
//! Java counterpart: the generated `ReceiptResponse` and `DownloadLink`, and the mapping in
//! `receipts/internal/web/ReceiptsController`. Properties are in the spec's order, which is Jackson's.

use ledgerly_shared::id::{ExpenseId, ReceiptId};
use serde::Serialize;

use crate::domain::Receipt;

/// `ReceiptResponse`. The storage key and the hash are never shown.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ReceiptResponse {
    pub id: ReceiptId,
    pub expense_id: ExpenseId,
    pub status: &'static str,
    pub detected_type: &'static str,
    pub size_bytes: i64,
    pub original_name: String,
    pub possible_duplicate: bool,
}

impl From<Receipt> for ReceiptResponse {
    fn from(receipt: Receipt) -> Self {
        Self {
            id: receipt.id,
            expense_id: receipt.expense_id,
            status: receipt.scan_status.as_str(),
            detected_type: receipt.detected_type.mime(),
            size_bytes: receipt.size_bytes,
            original_name: receipt.original_name,
            possible_duplicate: receipt.possible_duplicate,
        }
    }
}

/// `DownloadLink`: where to fetch the file, and until when.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DownloadLink {
    pub url: String,
    pub expires_at: String,
}
