//! Lesson 68: serde. Run `cargo test ex68`.

use serde::{Deserialize, Serialize};

#[derive(Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct CreateReceipt {
    pub file_name: String,
    pub size_bytes: u64,
    pub content_type: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReceiptResponse {
    pub receipt_id: String,
    pub scan_status: String,
    pub note: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_camel_case() {
        let body: CreateReceipt =
            serde_json::from_str(r#"{"fileName": "taxi.pdf", "sizeBytes": 2048}"#).unwrap();
        assert_eq!(body, CreateReceipt { file_name: "taxi.pdf".to_owned(), size_bytes: 2048, content_type: None });
    }

    #[test]
    fn rejects_unknown_keys() {
        let result = serde_json::from_str::<CreateReceipt>(r#"{"fileName": "a", "sizeBytes": 1, "admin": true}"#);
        assert!(result.is_err());
    }

    #[test]
    fn writes_camel_case_with_null() {
        let response = ReceiptResponse { receipt_id: "r1".to_owned(), scan_status: "PENDING".to_owned(), note: None };
        assert_eq!(
            serde_json::to_string(&response).unwrap(),
            r#"{"receiptId":"r1","scanStatus":"PENDING","note":null}"#
        );
    }
}
