//! Lesson 20: structs. Run `cargo test ex20`.

#[derive(Debug, Clone, PartialEq)]
pub struct Receipt {
    pub file_name: String,
    pub size_bytes: u64,
    pub scanned: bool,
}

/// Builds an unscanned receipt. Use field init shorthand for `file_name` and `size_bytes`.
pub fn upload(file_name: String, size_bytes: u64) -> Receipt {
    Receipt { file_name, size_bytes, scanned: false }
}

/// Returns a copy of `receipt` with `scanned: true`, using struct update syntax (`..receipt`).
pub fn mark_scanned(receipt: Receipt) -> Receipt {
    Receipt { scanned: true, ..receipt }
}

/// 2. A tuple struct wrapping the size in bytes.
pub struct Bytes(pub u64);

impl Bytes {
    pub fn kib(&self) -> u64 {
        self.0 / 1024
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upload_builds_an_unscanned_receipt() {
        let r = upload("taxi.pdf".to_owned(), 2048);
        assert_eq!(r.file_name, "taxi.pdf");
        assert_eq!(r.size_bytes, 2048);
        assert!(!r.scanned);
    }

    #[test]
    fn mark_scanned_keeps_the_rest() {
        let r = mark_scanned(upload("taxi.pdf".to_owned(), 2048));
        assert_eq!(r, Receipt { file_name: "taxi.pdf".to_owned(), size_bytes: 2048, scanned: true });
    }

    #[test]
    fn bytes_to_kib() {
        assert_eq!(Bytes(4096).kib(), 4);
    }
}
