//! The startup check that the database holds exactly the schema this build was made for (ADR 0006).
//!
//! Java has no counterpart: Ledgerly-Java runs Flyway itself at startup, and Flyway's `validate` refuses a
//! changed or unknown migration. Rust never runs DDL, so it repeats that validation against
//! `flyway_schema_history` before it serves anything.

use std::fmt;

use sqlx::PgPool;

use super::migrations::VendoredMigration;

/// Why the app refuses the database.
#[derive(Debug, thiserror::Error)]
pub enum SchemaError {
    #[error("the schema history could not be read: {0}")]
    Database(#[from] sqlx::Error),
    #[error(
        "The database has no flyway_schema_history table: it was never migrated.\n\n\
         Apply Ledgerly-Java's migrations with Flyway first (`just migrate`, or the `flyway` service in compose.yaml)."
    )]
    NeverMigrated,
    #[error("{0}")]
    Mismatch(Mismatch),
}

/// Every difference between `flyway_schema_history` and the vendored migrations, in version order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mismatch(pub Vec<Difference>);

/// One way in which the database differs from what this build expects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Difference {
    /// A vendored migration has no row in the history.
    NotApplied {
        version: &'static str,
        description: &'static str,
    },
    /// The row exists, but Flyway recorded that the script failed.
    Failed { version: String },
    /// The script that was applied is not the one vendored here.
    ChecksumChanged {
        version: String,
        expected: i32,
        recorded: Option<i32>,
    },
    /// The history holds a migration this build does not know: a newer Ledgerly, or a hand-made change.
    Unknown { version: Option<String>, script: String },
}

impl fmt::Display for Difference {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotApplied { version, description } => write!(f, "V{version} ({description}) has not been applied"),
            Self::Failed { version } => write!(f, "V{version} is recorded as failed"),
            Self::ChecksumChanged {
                version,
                expected,
                recorded: Some(recorded),
            } => write!(
                f,
                "V{version} was applied with checksum {recorded}, but this build's script has checksum {expected}"
            ),
            Self::ChecksumChanged {
                version,
                expected,
                recorded: None,
            } => write!(
                f,
                "V{version} was applied without a checksum, but this build's script has checksum {expected}"
            ),
            Self::Unknown {
                version: Some(version),
                script,
            } => write!(f, "V{version} ({script}) is not one of this build's migrations"),
            Self::Unknown { version: None, script } => {
                write!(f, "{script} is not one of this build's migrations")
            }
        }
    }
}

impl fmt::Display for Mismatch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("The database schema does not match the migrations this build was made for:\n")?;
        for difference in &self.0 {
            write!(f, "\n  - {difference}")?;
        }
        f.write_str(
            "\n\nLedgerly applies Ledgerly-Java's migrations unchanged and never changes the schema itself. \
             Run the Flyway step of this version (`just migrate`, or the `flyway` service in compose.yaml), \
             or deploy the version that matches the database.",
        )
    }
}

/// One row of `flyway_schema_history`, the columns the check needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryRow {
    pub version: Option<String>,
    pub script: String,
    pub checksum: Option<i32>,
    pub success: bool,
}

/// Refuses the database unless `flyway_schema_history` holds exactly the vendored migrations, each applied
/// successfully with the vendored script's checksum.
///
/// # Errors
///
/// [`SchemaError::NeverMigrated`] without a history table, [`SchemaError::Mismatch`] listing every difference,
/// or [`SchemaError::Database`] if the history cannot be read.
pub async fn verify(pool: &PgPool) -> Result<(), SchemaError> {
    let migrated = sqlx::query_scalar!(r#"select to_regclass('flyway_schema_history') is not null as "exists!""#)
        .fetch_one(pool)
        .await?;
    if !migrated {
        return Err(SchemaError::NeverMigrated);
    }
    let history = sqlx::query_as!(
        HistoryRow,
        "select version, script, checksum, success from flyway_schema_history order by installed_rank"
    )
    .fetch_all(pool)
    .await?;
    compare(VendoredMigration::all(), &history).map_err(SchemaError::Mismatch)
}

/// The comparison itself, without a database.
///
/// # Errors
///
/// Every difference, vendored migrations first in version order, then rows this build does not know.
pub fn compare(vendored: &[VendoredMigration], history: &[HistoryRow]) -> Result<(), Mismatch> {
    let mut differences = Vec::new();
    for migration in vendored {
        let row = history
            .iter()
            .find(|row| row.version.as_deref() == Some(migration.version));
        match row {
            None => differences.push(Difference::NotApplied {
                version: migration.version,
                description: migration.description,
            }),
            Some(row) if !row.success => differences.push(Difference::Failed {
                version: migration.version.to_owned(),
            }),
            Some(row) if row.checksum != Some(migration.checksum()) => differences.push(Difference::ChecksumChanged {
                version: migration.version.to_owned(),
                expected: migration.checksum(),
                recorded: row.checksum,
            }),
            Some(_) => {}
        }
    }
    for row in history {
        let known = vendored
            .iter()
            .any(|migration| row.version.as_deref() == Some(migration.version));
        if !known {
            differences.push(Difference::Unknown {
                version: row.version.clone(),
                script: row.script.clone(),
            });
        }
    }
    if differences.is_empty() {
        Ok(())
    } else {
        Err(Mismatch(differences))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn applied() -> Vec<HistoryRow> {
        VendoredMigration::all()
            .iter()
            .map(|migration| HistoryRow {
                version: Some(migration.version.to_owned()),
                script: migration.script.to_owned(),
                checksum: Some(migration.checksum()),
                success: true,
            })
            .collect()
    }

    #[test]
    fn exactly_the_vendored_migrations_pass() {
        assert_eq!(compare(VendoredMigration::all(), &applied()), Ok(()));
    }

    #[test]
    fn every_difference_is_reported_and_names_its_version() {
        let mut history = applied();
        history.retain(|row| row.version.as_deref() != Some("10"));
        for row in &mut history {
            match row.version.as_deref() {
                Some("3") => row.checksum = Some(1),
                Some("5") => row.success = false,
                _ => {}
            }
        }
        history.push(HistoryRow {
            version: Some("11".to_owned()),
            script: "V11__later.sql".to_owned(),
            checksum: Some(7),
            success: true,
        });

        let mismatch = compare(VendoredMigration::all(), &history).unwrap_err();
        let lines: Vec<String> = mismatch.0.iter().map(ToString::to_string).collect();
        assert_eq!(
            lines,
            [
                "V3 was applied with checksum 1, but this build's script has checksum 645491122",
                "V5 is recorded as failed",
                "V10 (external identity) has not been applied",
                "V11 (V11__later.sql) is not one of this build's migrations",
            ]
        );
        assert!(
            mismatch
                .to_string()
                .contains("\n  - V10 (external identity) has not been applied")
        );
    }
}
