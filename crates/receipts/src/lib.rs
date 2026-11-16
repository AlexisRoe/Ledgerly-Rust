//! The receipts module: upload, malware scan, storage and download of receipts.
//!
//! Java counterpart: `src/main/java/com/ledgerly/receipts/` in Ledgerly-Java at the commit in
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
use ledgerly_shared::storage::ObjectStorage;
use ledgerly_shared::web::Routes;
use sqlx::PgPool;

use crate::application::{Context, ListReceipts, UploadReceipt};
use crate::web::ReceiptsApi;

/// The receipts module as the app wires it.
#[derive(Debug, Clone)]
pub struct Receipts {
    api: ReceiptsApi,
}

impl Receipts {
    /// Files go to `storage`; who may do what is asked of `expenses`. Uploads are capped at
    /// `limits.max_upload_size`.
    #[must_use]
    pub fn new(
        pool: &PgPool,
        clock: &Arc<dyn Clock>,
        events: &Arc<dyn Publisher>,
        storage: &Arc<dyn ObjectStorage>,
        expenses: ExpensesFacade,
        limits: &Limits,
    ) -> Self {
        let context = Context {
            pool: pool.clone(),
            clock: clock.clone(),
            events: events.clone(),
            storage: storage.clone(),
            expenses,
        };
        Self {
            api: ReceiptsApi {
                upload: UploadReceipt::new(context.clone(), limits.max_upload_size),
                list: ListReceipts::new(context),
            },
        }
    }

    /// The receipts endpoints.
    #[must_use]
    pub fn routes(&self) -> Routes {
        self.api.clone().routes()
    }
}
