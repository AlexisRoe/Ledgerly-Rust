//! The imports module's event. Counts only, never row data (Java's ADR 0001).
//!
//! Java counterpart: `imports/events/ImportCompleted.java`, and the module-internal
//! `internal/application/ImportQueued.java`, which starts the job through the outbox.

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

/// An import was accepted and waits for its job. Internal: only the module's own runner listens. Java:
/// `ImportQueued`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ImportQueued {
    pub event_id: Uuid,
    pub team_id: TeamId,
    pub import_id: ImportId,
    pub occurred_at: DateTime<Utc>,
}

/// The `event_type` of [`ImportQueued`].
pub(crate) const IMPORT_QUEUED: &str = "ledgerly-rust:imports.ImportQueued";

impl Event for ImportQueued {
    fn event_id(&self) -> Uuid {
        self.event_id
    }

    fn occurred_at(&self) -> DateTime<Utc> {
        self.occurred_at
    }

    fn event_type(&self) -> &'static str {
        IMPORT_QUEUED
    }

    fn payload(&self) -> serde_json::Result<String> {
        serde_json::to_string(self)
    }
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
