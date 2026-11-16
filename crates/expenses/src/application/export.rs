//! Java counterpart: `expenses/internal/application/ExportExpenses.java`.

use std::sync::Arc;

use ledgerly_shared::id::{TeamId, UserId};
use ledgerly_shared::version::Versioned;
use sqlx::PgPool;

use super::filters::{self, Filter};
use super::{MembershipLookup, UseCaseError, access};
use crate::domain::Expense;
use crate::persistence::{self, Criteria, Direction, Order, SortField};

/// Rows per page, each page read in its own short transaction. Java: `ExportExpenses.PAGE`.
const PAGE: u32 = 500;

/// Newest first; the id breaks ties. Java: `incurredOn desc, id desc`.
const ORDER: Order = Order {
    field: SortField::IncurredOn,
    direction: Direction::Descending,
};

/// The expenses matching the filters, handed out page by page: never more than `max-export-rows`, never the
/// whole result in memory, and no connection held while the client downloads.
#[derive(Debug, Clone)]
pub(crate) struct ExportExpenses {
    pool: PgPool,
    memberships: Arc<dyn MembershipLookup>,
    max_export_rows: u32,
}

/// How many rows matched (`total`, possibly more than are sent), whether the export is cut at the cap, and
/// the rows.
#[derive(Debug)]
pub(crate) struct Export {
    pub total: i64,
    pub truncated: bool,
    pub pages: ExportPages,
}

/// The rows of an export, one page at a time. Java: the `Supplier<List<ExpenseView>>` of `Export`.
#[derive(Debug)]
pub(crate) struct ExportPages {
    pool: PgPool,
    criteria: Criteria,
    first: Option<Vec<Versioned<Expense>>>,
    page_number: i64,
    sent: u32,
    cap: u32,
    total: i64,
}

impl ExportExpenses {
    pub(crate) fn new(pool: PgPool, memberships: Arc<dyn MembershipLookup>, max_export_rows: u32) -> Self {
        Self {
            pool,
            memberships,
            max_export_rows,
        }
    }

    /// The total and the first page; the rest is read as the response is written.
    ///
    /// # Errors
    ///
    /// Not a teammate (404); the list's filter rules (400); a database failure.
    pub(crate) async fn execute(&self, team: TeamId, caller: UserId, filter: Filter) -> Result<Export, UseCaseError> {
        let criteria = {
            let mut connection = self.pool.acquire().await?;
            let actor = access::actor(&mut connection, self.memberships.as_ref(), team, caller).await?;
            filters::build(actor, team, filter)?
        };
        let mut transaction = read_only(&self.pool).await?;
        let first = persistence::list(&mut transaction, &criteria, ORDER, 0, PAGE).await?;
        // Spring Data's `getPage`: no count query when the first page shows where the end is.
        let returned = i64::try_from(first.len()).unwrap_or(i64::MAX);
        let total = if returned < i64::from(PAGE) {
            returned
        } else {
            persistence::count(&mut transaction, &criteria).await?
        };
        transaction.commit().await?;
        let cap = self.max_export_rows;
        Ok(Export {
            total,
            truncated: total > i64::from(cap),
            pages: ExportPages {
                pool: self.pool.clone(),
                criteria,
                first: Some(first),
                page_number: 0,
                sent: 0,
                cap,
                total,
            },
        })
    }
}

impl ExportPages {
    /// The next rows, or none once everything (or the cap) has been sent.
    ///
    /// # Errors
    ///
    /// A database failure while reading a page.
    pub(crate) async fn next(&mut self) -> Result<Vec<Versioned<Expense>>, UseCaseError> {
        let mut rows = match self.first.take() {
            Some(rows) => rows,
            None => {
                let page = i64::from(PAGE);
                if self.sent >= self.cap || self.page_number * page + page >= self.total {
                    return Ok(Vec::new());
                }
                self.page_number += 1;
                let mut transaction = read_only(&self.pool).await?;
                let rows =
                    persistence::list(&mut transaction, &self.criteria, ORDER, self.page_number * page, PAGE).await?;
                transaction.commit().await?;
                rows
            }
        };
        let room = usize::try_from(self.cap.saturating_sub(self.sent)).unwrap_or(usize::MAX);
        rows.truncate(room);
        self.sent = self.sent.saturating_add(u32::try_from(rows.len()).unwrap_or(u32::MAX));
        Ok(rows)
    }
}

async fn read_only(pool: &PgPool) -> Result<sqlx::Transaction<'static, sqlx::Postgres>, UseCaseError> {
    let mut transaction = pool.begin().await?;
    sqlx::query("set transaction read only")
        .execute(&mut *transaction)
        .await?;
    Ok(transaction)
}
