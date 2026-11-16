//! A receipt, its scan status, the file types it may have, and the rules for its names.
//!
//! Java counterpart: `receipts/internal/domain/` (`Receipt`, `ScanStatus`), `UploadReceipt.displayName` and
//! `ALLOWED_TYPES`, `FileNames`, and the part of Apache Tika's magic-byte detection that decides them.

use std::fmt;
use std::str::FromStr;

use chrono::{DateTime, Utc};
use ledgerly_shared::id::{ExpenseId, ReceiptId, TeamId, UserId};
use ledgerly_shared::text::strip;

/// Where a receipt is in the malware check: `PendingScan`, then `Accepted` (clean) or `Quarantined`
/// (infected). Only an accepted receipt is ever served.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanStatus {
    PendingScan,
    Accepted,
    Quarantined,
}

impl ScanStatus {
    /// The name as Java, the database and the API write it.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::PendingScan => "PENDING_SCAN",
            Self::Accepted => "ACCEPTED",
            Self::Quarantined => "QUARANTINED",
        }
    }
}

impl fmt::Display for ScanStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for ScanStatus {
    type Err = UnknownValue;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        [Self::PendingScan, Self::Accepted, Self::Quarantined]
            .into_iter()
            .find(|status| status.as_str() == text)
            .ok_or(UnknownValue)
    }
}

/// A stored value that is not one this module writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("unknown value")]
pub struct UnknownValue;

/// The three file types a receipt may have, as detected from its bytes. Java: `UploadReceipt.ALLOWED_TYPES`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetectedType {
    Pdf,
    Jpeg,
    Png,
}

impl DetectedType {
    /// The MIME type stored in `detected_type` and shown as `detectedType`.
    #[must_use]
    pub const fn mime(self) -> &'static str {
        match self {
            Self::Pdf => "application/pdf",
            Self::Jpeg => "image/jpeg",
            Self::Png => "image/png",
        }
    }

    /// The extension of a download name. Java: `FileNames.EXTENSIONS`.
    #[must_use]
    pub const fn extension(self) -> &'static str {
        match self {
            Self::Pdf => "pdf",
            Self::Jpeg => "jpg",
            Self::Png => "png",
        }
    }

    /// The first bytes of a file decide its type; its name and the type the client declared are never looked
    /// at. `None`: none of the three.
    ///
    /// These are the rules of Apache Tika 4.2 (`tika-mimetypes.xml`) that match from the first byte, at
    /// priority 50 or 40, which is what Java's `new Tika().detect(stream)` finds for these types:
    /// - PDF: `%PDF-`, also after a UTF-8 byte order mark, or after up to 50 lines that are empty or `%%`
    ///   comments of at most 150 characters, followed by `%PDF-1.` or `%PDF-2.` (print job tickets);
    /// - JPEG: `FF D8 FF`;
    /// - PNG: `89 50 4E 47 0D 0A 1A 0A`.
    ///
    /// Tika's weaker PDF rules, `%PDF-1.` anywhere in the first 512 bytes, are not copied: they only win when
    /// no other of Tika's 1,500 types matches with a higher priority, and that cannot be decided without them.
    /// Such a file is refused here (learning log M7, open question).
    #[must_use]
    pub fn detect(head: &[u8]) -> Option<Self> {
        if head.starts_with(b"%PDF-") || head.starts_with(b"\xef\xbb\xbf%PDF-") || after_job_ticket(head) {
            Some(Self::Pdf)
        } else if head.starts_with(&[0xFF, 0xD8, 0xFF]) {
            Some(Self::Jpeg)
        } else if head.starts_with(b"\x89PNG\r\n\x1a\n") {
            Some(Self::Png)
        } else {
            None
        }
    }
}

impl FromStr for DetectedType {
    type Err = UnknownValue;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        [Self::Pdf, Self::Jpeg, Self::Png]
            .into_iter()
            .find(|kind| kind.mime() == text)
            .ok_or(UnknownValue)
    }
}

/// How many bytes [`DetectedType::detect`] needs: Tika gives a regular expression the first 8 KiB.
pub const DETECTION_WINDOW: usize = 8 * 1024;

/// Tika's `(?>(?:%%[^\r\n]{0,148})?(?:\r\n|[\r\n])){1,50}%PDF-[12]\.` at offset 0, without a regex: the
/// group is atomic, so each line is consumed one way only, and the header may follow any of the 1 to 50 lines.
fn after_job_ticket(head: &[u8]) -> bool {
    let head = head.get(..DETECTION_WINDOW).unwrap_or(head);
    let mut position = 0;
    for _ in 0..50 {
        let Some(next) = ticket_line(head, position) else {
            return false;
        };
        position = next;
        let rest = head.get(position..).unwrap_or_default();
        if rest.starts_with(b"%PDF-1.") || rest.starts_with(b"%PDF-2.") {
            return true;
        }
    }
    false
}

/// One line of a job ticket from `start`: optionally `%%` and up to 148 characters other than CR and LF, then
/// a line break (CR LF, CR or LF). The end of the line, or `None`.
fn ticket_line(head: &[u8], start: usize) -> Option<usize> {
    let rest = head.get(start..)?;
    let comment = if rest.starts_with(b"%%") {
        let text = rest.get(2..).unwrap_or_default();
        let length = text.iter().take_while(|&&b| b != b'\r' && b != b'\n').count();
        // Longer than 148: the comment cannot end in a line break here; the optional group is skipped.
        if length <= 148 { 2 + length } else { 0 }
    } else {
        0
    };
    let after = rest.get(comment..)?;
    let line_break = if after.starts_with(b"\r\n") {
        2
    } else if after.starts_with(b"\r") || after.starts_with(b"\n") {
        1
    } else {
        return None;
    };
    Some(start + comment + line_break)
}

/// What is kept of the client's file name: control characters dropped, Java-stripped, at most 255 UTF-16
/// units, `receipt` if nothing is left. Metadata only; it never becomes a path. Java:
/// `UploadReceipt.displayName`.
#[must_use]
pub fn display_name(original: &str) -> String {
    // Java's `\p{Cntrl}`: the C0 controls and DEL, nothing else.
    let without_controls: String = original
        .chars()
        .filter(|c| !matches!(*c, '\u{0}'..='\u{1F}' | '\u{7F}'))
        .collect();
    let name = strip(&without_controls);
    if name.is_empty() {
        return "receipt".to_owned();
    }
    first_utf16_units(name, 255)
}

/// Java's `substring(0, limit)` on a string longer than `limit` UTF-16 units. A character whose two units
/// would be split leaves Java a lone surrogate, which the Postgres driver stores as `?`; so does this.
fn first_utf16_units(text: &str, limit: usize) -> String {
    let mut kept = String::new();
    let mut units = 0;
    for c in text.chars() {
        let width = c.len_utf16();
        if units + width > limit {
            if units < limit {
                kept.push('?');
            }
            break;
        }
        units += width;
        kept.push(c);
    }
    kept
}

/// A name safe to put in a download header: only letters, digits, dash and underscore, no directory part, no
/// leading separators, at most 80 characters before the extension, and the extension of the detected type,
/// never the claimed one. Java: `FileNames.sanitize`.
#[must_use]
pub fn download_name(original: &str, detected: Option<DetectedType>) -> String {
    let name = original
        .rfind(['/', '\\'])
        .map_or(original, |slash| original.get(slash + 1..).unwrap_or_default());
    // Java: `dot > 0`; a name that starts with its only dot keeps it as part of the base.
    let base = match name.rfind('.') {
        Some(dot) if dot > 0 => name.get(..dot).unwrap_or_default(),
        _ => name,
    };
    let replaced: String = base
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let mut base = replaced.trim_start_matches(['_', '-']).to_owned();
    // Only ASCII is left, so bytes are characters.
    base.truncate(80);
    if base.is_empty() {
        base = "receipt".to_owned();
    }
    format!("{base}.{}", detected.map_or("bin", DetectedType::extension))
}

/// A receipt as stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Receipt {
    pub id: ReceiptId,
    pub team_id: TeamId,
    pub expense_id: ExpenseId,
    /// Where the file lives in the object store: random, never shown to a client.
    pub storage_key: String,
    /// The uploader's file name after [`display_name`].
    pub original_name: String,
    pub detected_type: DetectedType,
    pub size_bytes: i64,
    /// Lower-case hex SHA-256 of the content.
    pub sha256: String,
    pub scan_status: ScanStatus,
    /// The same content is attached to another expense of the team.
    pub possible_duplicate: bool,
    pub uploaded_by: UserId,
    pub created_at: DateTime<Utc>,
}

impl Receipt {
    /// Only an accepted receipt is served.
    #[must_use]
    pub fn is_downloadable(&self) -> bool {
        self.scan_status == ScanStatus::Accepted
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Java: `FileNamesTest.keepsOnlySafeCharactersAndTheRealExtension`.
    #[test]
    fn download_names_keep_only_safe_characters_and_the_real_extension() {
        let cases = [
            ("lunch receipt.pdf", DetectedType::Pdf, "lunch_receipt.pdf"),
            ("../../etc/passwd", DetectedType::Pdf, "passwd.pdf"),
            (r"..\..\windows\evil.pdf", DetectedType::Pdf, "evil.pdf"),
            (".hidden.png", DetectedType::Png, "hidden.png"),
            ("scan.exe", DetectedType::Pdf, "scan.pdf"),
            ("photo.PDF", DetectedType::Jpeg, "photo.jpg"),
            (r#"na"me;rm -rf.pdf"#, DetectedType::Pdf, "na_me_rm_-rf.pdf"),
            ("___.pdf", DetectedType::Pdf, "receipt.pdf"),
        ];
        for (original, detected, expected) in cases {
            assert_eq!(download_name(original, Some(detected)), expected, "{original}");
        }
    }

    /// Java: `FileNamesTest.neverContainsPathSeparatorsQuotesOrControlCharacters`.
    #[test]
    fn download_names_never_contain_separators_quotes_or_control_characters() {
        let name = download_name("a/b\\c\"d\r\ne\u{0}f.pdf", Some(DetectedType::Pdf));
        let (base, extension) = name.rsplit_once('.').unwrap();
        assert_eq!(extension, "pdf");
        assert!(!base.is_empty());
        assert!(
            base.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-'),
            "{name}"
        );
    }

    /// Java: `FileNamesTest.longNamesAreShortened`.
    #[test]
    fn long_download_names_are_shortened() {
        assert!(download_name(&format!("{}.pdf", "x".repeat(500)), Some(DetectedType::Pdf)).len() <= 84);
    }

    #[test]
    fn a_character_outside_ascii_becomes_one_underscore_and_an_unknown_type_bin() {
        assert_eq!(download_name("a😀b.pdf", None), "a_b.bin");
    }

    #[test]
    fn display_names_drop_control_characters_strip_and_default() {
        assert_eq!(display_name("nice\tname.pdf"), "nicename.pdf");
        assert_eq!(display_name("../../etc/passwd"), "../../etc/passwd");
        assert_eq!(display_name("  \u{1}  "), "receipt");
        assert_eq!(display_name(""), "receipt");
        // Java's `strip()` leaves a no-break space, and `\p{Cntrl}` leaves U+0085.
        assert_eq!(display_name("\u{a0}a\u{85}"), "\u{a0}a\u{85}");
    }

    #[test]
    fn display_names_are_cut_at_255_utf16_units() {
        assert_eq!(display_name(&"a".repeat(300)), "a".repeat(255));
        // 254 units, then a character of two: Java keeps half of it, which is stored as `?`.
        assert_eq!(
            display_name(&format!("{}😀tail", "a".repeat(254))),
            format!("{}?", "a".repeat(254))
        );
        assert_eq!(
            display_name(&format!("{}😀tail", "a".repeat(253))),
            format!("{}😀", "a".repeat(253))
        );
    }

    #[test]
    fn the_type_comes_from_the_first_bytes() {
        let cases: [(&[u8], Option<DetectedType>); 12] = [
            (b"%PDF-1.4\n", Some(DetectedType::Pdf)),
            (b"\xef\xbb\xbf%PDF-1.7", Some(DetectedType::Pdf)),
            (b"%%Job ticket\r\n%%Copies: 2\n\n%PDF-1.4", Some(DetectedType::Pdf)),
            (b"\n%PDF-2.0", Some(DetectedType::Pdf)),
            (&[0xFF, 0xD8, 0xFF, 0xE0], Some(DetectedType::Jpeg)),
            (b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR", Some(DetectedType::Png)),
            (b"%PDF", None),
            (b"MZ\x90\0", None),
            (b"<html><script>alert(1)</script></html>", None),
            (b"hello", None),
            // The header must follow the ticket's line break, and a line is a comment or empty.
            (b"%%comment %PDF-1.4", None),
            (b"text\n%PDF-1.4", None),
        ];
        for (head, expected) in cases {
            assert_eq!(DetectedType::detect(head), expected, "{head:?}");
        }
    }

    #[test]
    fn a_job_ticket_has_at_most_50_lines_of_at_most_150_characters() {
        let lines = |count: usize, length: usize| {
            let mut head = format!("%%{}\n", "x".repeat(length - 2)).repeat(count).into_bytes();
            head.extend_from_slice(b"%PDF-1.4");
            head
        };
        assert_eq!(DetectedType::detect(&lines(50, 150)), Some(DetectedType::Pdf));
        assert_eq!(DetectedType::detect(&lines(51, 150)), None);
        assert_eq!(DetectedType::detect(&lines(1, 151)), None);
    }

    #[test]
    fn statuses_and_types_read_back_what_is_written() {
        for status in [ScanStatus::PendingScan, ScanStatus::Accepted, ScanStatus::Quarantined] {
            assert_eq!(status.as_str().parse(), Ok(status));
        }
        for kind in [DetectedType::Pdf, DetectedType::Jpeg, DetectedType::Png] {
            assert_eq!(kind.mime().parse(), Ok(kind));
        }
        assert_eq!("text/plain".parse::<DetectedType>(), Err(UnknownValue));
    }
}
