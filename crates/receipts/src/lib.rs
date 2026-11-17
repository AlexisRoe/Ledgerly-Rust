//! The receipts module: upload, malware scan, storage and download of receipts.
//!
//! Java counterpart: `src/main/java/com/ledgerly/receipts/` in Ledgerly-Java at the commit in
//! `vendor/ledgerly-java/SOURCE`.

#![forbid(unsafe_code)]

mod application;
mod domain;
pub mod events;
mod persistence;
mod scanner;
mod web;

use std::sync::Arc;
use std::time::Duration;

use ledgerly_expenses::{ExpensesFacade, ReceiptChecks};
use ledgerly_shared::clock::Clock;
use ledgerly_shared::config::{ClamavConfig, Limits};
use ledgerly_shared::events::{Listener, Publisher};
use ledgerly_shared::storage::ObjectStorage;
use ledgerly_shared::web::Routes;
use sqlx::PgPool;

use crate::application::{
    CleanupListener, Context, DeleteReceipt, DownloadReceipt, ExpenseDeletedCleanup, ListReceipts, RETRY_DELAYS,
    ReceiptCounts, ScanListener, ScanReceipt, UploadReceipt,
};
use crate::scanner::{ClamAvScanner, MalwareScanner};
use crate::web::ReceiptsApi;

/// What the receipts module takes from the rest of the app.
#[derive(Debug, Clone)]
pub struct Modules<'a> {
    pub pool: &'a PgPool,
    pub clock: &'a Arc<dyn Clock>,
    pub events: &'a Arc<dyn Publisher>,
    /// Who may see an expense and change its receipts.
    pub expenses: ExpensesFacade,
}

/// The receipts module as the app wires it.
#[derive(Debug, Clone)]
pub struct Receipts {
    api: ReceiptsApi,
    scan: ScanReceipt,
    cleanup: ExpenseDeletedCleanup,
}

impl Receipts {
    /// Files go to `storage` and are scanned by the clamd at `clamav`; who may do what is asked of
    /// `expenses`. Uploads are capped at `limits.max_upload_size`.
    #[must_use]
    pub fn new(modules: Modules<'_>, storage: &Arc<dyn ObjectStorage>, clamav: &ClamavConfig, limits: &Limits) -> Self {
        let scanner: Arc<dyn MalwareScanner> = Arc::new(ClamAvScanner::new(clamav.clone()));
        Self::assemble(modules, storage, scanner, RETRY_DELAYS, limits)
    }

    fn assemble(
        modules: Modules<'_>,
        storage: &Arc<dyn ObjectStorage>,
        scanner: Arc<dyn MalwareScanner>,
        retry_delays: [Duration; 3],
        limits: &Limits,
    ) -> Self {
        let context = Context {
            pool: modules.pool.clone(),
            clock: modules.clock.clone(),
            events: modules.events.clone(),
            storage: storage.clone(),
            expenses: modules.expenses,
        };
        let scan = ScanReceipt::new(context.clone(), scanner, retry_delays);
        Self {
            api: ReceiptsApi {
                upload: UploadReceipt::new(context.clone(), limits.max_upload_size),
                list: ListReceipts::new(context.clone()),
                download: DownloadReceipt::new(context.clone()),
                delete: DeleteReceipt::new(context.clone()),
            },
            scan,
            cleanup: ExpenseDeletedCleanup::new(context),
        }
    }

    /// The receipts endpoints.
    #[must_use]
    pub fn routes(&self) -> Routes {
        self.api.clone().routes()
    }

    /// Receipts' side of the submit rule, for the expenses module. Java: `ReceiptChecksImpl`.
    #[must_use]
    pub fn checks() -> Arc<dyn ReceiptChecks> {
        Arc::new(ReceiptCounts)
    }

    /// The outbox listeners: the malware scan of an uploaded receipt, and the removal of a deleted expense's
    /// receipts. Java: `ScanReceipt.on` and `ExpenseDeletedCleanup`.
    #[must_use]
    pub fn listeners(&self) -> Vec<Arc<dyn Listener>> {
        vec![
            Arc::new(ScanListener(self.scan.clone())),
            Arc::new(CleanupListener(self.cleanup.clone())),
        ]
    }
}
