//! The imports module's event. Counts only, never row data (Java's ADR 0001).
//!
//! Java counterpart: `imports/events/ImportCompleted.java`. Java's module-internal `ImportQueued`, which starts
//! the job, has no counterpart: until the outbox (M9.2) the upload starts the job itself once it has committed.

use chrono::{DateTime, Utc};
use ledgerly_shared::events::Event;
use ledgerly_shared::id::{ImportId, TeamId, UserId};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// An import job finished. `failed`: it stopped early and can be restarted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportCompleted {
    pub event_id: Uuid,
    pub team_id: TeamId,
    /// The user who started the import.
    pub actor_id: UserId,
    pub import_id: ImportId,
    pub imported_rows: i64,
    pub skipped_rows: i64,
    pub failed: bool,
    pub occurred_at: DateTime<Utc>,
}

impl Event for ImportCompleted {
    fn event_id(&self) -> Uuid {
        self.event_id
    }

    fn occurred_at(&self) -> DateTime<Utc> {
        self.occurred_at
    }

    fn event_type(&self) -> &'static str {
        "ledgerly-rust:imports.ImportCompleted"
    }

    fn payload(&self) -> serde_json::Result<String> {
        serde_json::to_string(self)
    }
}
