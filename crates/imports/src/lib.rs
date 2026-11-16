//! The imports module: CSV files that become many draft expenses, in the background.
//!
//! Java counterpart: `src/main/java/com/ledgerly/imports/` in Ledgerly-Java at the commit in
//! `vendor/ledgerly-java/SOURCE`.

#![forbid(unsafe_code)]

mod application;
mod domain;
pub mod events;
mod persistence;
mod web;

use std::sync::Arc;

use ledgerly_expenses::ExpensesFacade;
use ledgerly_shared::clock::Clock;
use ledgerly_shared::config::Limits;
use ledgerly_shared::events::Publisher;
use ledgerly_shared::id::{ImportId, TeamId};
use ledgerly_shared::storage::ObjectStorage;
use ledgerly_shared::web::Routes;
use ledgerly_teams::TeamsFacade;
use sqlx::PgPool;

use crate::application::{Context, Drafts, GetImport, ImportRunner, UploadImport};
use crate::web::ImportsApi;

/// The imports module as the app wires it.
#[derive(Debug, Clone)]
pub struct Imports {
    api: ImportsApi,
    runner: ImportRunner,
}

impl Imports {
    /// Files go to `storage`; drafts are created through `expenses`; teammates come from `teams`.
    #[must_use]
    pub fn new(
        pool: &PgPool,
        clock: &Arc<dyn Clock>,
        events: &Arc<dyn Publisher>,
        storage: &Arc<dyn ObjectStorage>,
        teams: TeamsFacade,
        expenses: ExpensesFacade,
        limits: &Limits,
    ) -> Self {
        let drafts: Arc<dyn Drafts> = Arc::new(expenses);
        let context = Context {
            pool: pool.clone(),
            clock: clock.clone(),
            events: events.clone(),
            storage: storage.clone(),
            teams,
            drafts,
            limits: *limits,
        };
        Self::with(context)
    }

    pub(crate) fn with(context: Context) -> Self {
        let runner = ImportRunner::new(context.clone());
        Self {
            api: ImportsApi {
                upload: UploadImport::new(context.clone(), runner.clone()),
                get: GetImport::new(context),
            },
            runner,
        }
    }

    /// Resumes a `FAILED` import after its last committed chunk, and runs it to its end. `false` when the
    /// import does not exist in the team or has not failed. Java: `ImportRunner.restart`, public for a future
    /// admin tool; no endpoint calls it.
    pub async fn restart(&self, team: TeamId, id: ImportId) -> bool {
        self.runner.restart(team, id).await
    }

    /// The imports endpoints.
    #[must_use]
    pub fn routes(&self) -> Routes {
        self.api.clone().routes()
    }
}
