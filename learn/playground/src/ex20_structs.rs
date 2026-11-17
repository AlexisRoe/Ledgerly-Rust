//! Lesson 20: structs. Run `cargo test ex20`.

// 1. Define a public struct `Receipt` with three public fields:
//      file_name: String
//      size_bytes: u64
//      scanned: bool
//    and put `#[derive(Debug, Clone, PartialEq)]` above it (needed by the tests).
//    Until you do, the functions below won't compile, so they're commented out. Uncomment them after step 1.

// /// Builds an unscanned receipt. Use field init shorthand for `file_name` and `size_bytes`.
// pub fn upload(file_name: String, size_bytes: u64) -> Receipt {
//     todo!()
// }
//
// /// Returns a copy of `receipt` with `scanned: true`, using struct update syntax (`..receipt`).
// pub fn mark_scanned(receipt: Receipt) -> Receipt {
//     todo!()
// }

/// 2. A tuple struct wrapping the size in bytes. Finish `kib` (bytes / 1024) by reading the field `.0`.
pub struct Bytes(pub u64);

impl Bytes {
    pub fn kib(&self) -> u64 {
        todo!()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Uncomment these two tests together with the functions above.
    // #[test]
    // fn upload_builds_an_unscanned_receipt() {
    //     let r = upload("taxi.pdf".to_owned(), 2048);
    //     assert_eq!(r.file_name, "taxi.pdf");
    //     assert_eq!(r.size_bytes, 2048);
    //     assert!(!r.scanned);
    // }
    //
    // #[test]
    // fn mark_scanned_keeps_the_rest() {
    //     let r = mark_scanned(upload("taxi.pdf".to_owned(), 2048));
    //     assert_eq!(r, Receipt { file_name: "taxi.pdf".to_owned(), size_bytes: 2048, scanned: true });
    // }

    #[test]
    fn bytes_to_kib() {
        assert_eq!(Bytes(4096).kib(), 4);
    }
}
