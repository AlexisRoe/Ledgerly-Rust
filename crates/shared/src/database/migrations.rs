//! Ledgerly-Java's Flyway migrations as this build knows them, and the checksum Flyway records for each.

/// One versioned migration from `vendor/ledgerly-java/src/main/resources/db/migration`, embedded at build time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VendoredMigration {
    /// As Flyway writes it into `flyway_schema_history.version`: `10` for `V10__external_identity.sql`.
    pub version: &'static str,
    /// As Flyway writes it into `description`: underscores become spaces.
    pub description: &'static str,
    /// The file name, as in `script`.
    pub script: &'static str,
    pub sql: &'static str,
}

impl VendoredMigration {
    /// Every vendored migration, ordered by version.
    #[must_use]
    pub fn all() -> &'static [Self] {
        include!(concat!(env!("OUT_DIR"), "/migrations.rs"))
    }

    /// The checksum Flyway stores for this script.
    #[must_use]
    pub fn checksum(&self) -> i32 {
        flyway_checksum(self.sql)
    }
}

/// Flyway's checksum of an SQL script (`ChecksumCalculator`): a CRC-32 fed with the UTF-8 bytes of each line,
/// without its line break, after a byte-order mark is removed from the first line. Java's `int` is signed, so
/// is the result.
///
/// Lines end as Java's `BufferedReader.readLine` sees them: at `\n`, `\r` or `\r\n`. A file saved with Windows
/// line breaks therefore has the same checksum as one with Unix line breaks, which is what Flyway intends.
#[must_use]
pub fn flyway_checksum(sql: &str) -> i32 {
    const CRC_32: crc::Crc<u32> = crc::Crc::<u32>::new(&crc::CRC_32_ISO_HDLC);
    let mut digest = CRC_32.digest();
    for line in java_lines(sql.strip_prefix('\u{feff}').unwrap_or(sql)) {
        digest.update(line.as_bytes());
    }
    // The same 32 bits, read as Java's signed `int`.
    i32::from_ne_bytes(digest.finalize().to_ne_bytes())
}

/// The lines of `text` as `BufferedReader.readLine` returns them: no terminators, and no empty line after a
/// final line break.
fn java_lines(text: &str) -> impl Iterator<Item = &str> {
    let mut rest = text;
    std::iter::from_fn(move || {
        if rest.is_empty() {
            return None;
        }
        match rest.find(['\n', '\r']) {
            None => Some(std::mem::take(&mut rest)),
            Some(end) => {
                let line = rest.get(..end).unwrap_or_default();
                let after = rest.get(end..).unwrap_or_default();
                rest = after
                    .strip_prefix("\r\n")
                    .or_else(|| after.strip_prefix('\n'))
                    .or_else(|| after.strip_prefix('\r'))
                    .unwrap_or_default();
                Some(line)
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The values Flyway 12 wrote into `flyway_schema_history` for the vendored scripts (read from a database
    /// it migrated). If Ledgerly-Java's migrations are re-vendored, this list changes with them.
    const RECORDED_BY_FLYWAY: [(&str, i32); 10] = [
        ("1", 40_139_244),
        ("2", -1_768_363_514),
        ("3", 645_491_122),
        ("4", -1_876_450_748),
        ("5", 1_693_982_363),
        ("6", 808_253_596),
        ("7", -1_738_398_052),
        ("8", 1_053_078_002),
        ("9", -381_896_485),
        ("10", -600_795_581),
    ];

    #[test]
    fn the_checksums_are_the_ones_flyway_recorded() {
        let computed: Vec<(&str, i32)> = VendoredMigration::all()
            .iter()
            .map(|migration| (migration.version, migration.checksum()))
            .collect();
        assert_eq!(computed, RECORDED_BY_FLYWAY);
    }

    #[test]
    fn versions_are_ordered_numerically_and_named_as_flyway_names_them() {
        let all = VendoredMigration::all();
        assert_eq!(all.first().map(|m| (m.version, m.description)), Some(("1", "expenses")));
        assert_eq!(
            all.last().map(|m| (m.version, m.description, m.script)),
            Some(("10", "external identity", "V10__external_identity.sql"))
        );
    }

    #[test]
    fn line_breaks_and_a_byte_order_mark_do_not_change_the_checksum() {
        let unix = flyway_checksum("create table a (id int);\nselect 1;\n");
        assert_eq!(flyway_checksum("create table a (id int);\r\nselect 1;\r\n"), unix);
        assert_eq!(flyway_checksum("create table a (id int);\rselect 1;"), unix);
        assert_eq!(flyway_checksum("\u{feff}create table a (id int);\nselect 1;"), unix);
    }

    #[test]
    fn empty_lines_count_for_nothing_but_content_does() {
        // An empty line feeds no bytes into the CRC, as in Flyway.
        assert_eq!(flyway_checksum("a\n\nb"), flyway_checksum("a\nb"));
        assert_ne!(flyway_checksum("a\nb"), flyway_checksum("a\nc"));
        assert_eq!(flyway_checksum(""), 0);
    }

    #[test]
    fn java_lines_matches_buffered_reader() {
        assert_eq!(java_lines("a\r\nb\rc\nd").collect::<Vec<_>>(), ["a", "b", "c", "d"]);
        assert_eq!(java_lines("a\n").collect::<Vec<_>>(), ["a"]);
        assert_eq!(java_lines("\n\na").collect::<Vec<_>>(), ["", "", "a"]);
        assert_eq!(java_lines("").count(), 0);
    }
}
