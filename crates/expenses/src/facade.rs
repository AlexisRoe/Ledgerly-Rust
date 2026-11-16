//! The expenses module's API for modules that hang data off an expense (receipts now, imports in M8).
//!
//! Java counterpart: `expenses/ExpensesFacade.java` and `internal/application/ExpensesFacadeImpl.java`. Both
//! checks fail with the usual API answers instead of returning flags, so a caller cannot forget to check. They
//! run on the caller's connection, so that the check and what the caller does next can share a transaction.

use std::sync::Arc;

use ledgerly_shared::clock::{Clock, SystemClock};
use ledgerly_shared::config::Limits;
use ledgerly_shared::id::{ExpenseId, TeamId, UserId};
use ledgerly_shared::web::Problem;
use ledgerly_teams::TeamsFacade;
use sqlx::PgConnection;

use crate::application::{MembershipLookup, TeamsMembershipLookup, UseCaseError, access};
use crate::domain::{Expense, ExpenseError, NewExpense};
use crate::persistence;
use crate::web::problem::failure;

/// Who may see an expense, who may change its receipts, and the one way other modules create expenses.
#[derive(Debug, Clone)]
pub struct ExpensesFacade {
    memberships: Arc<dyn MembershipLookup>,
    clock: Arc<dyn Clock>,
    max_batch_size: u32,
}

/// The first rule a would-be expense breaks, as a client can be shown it. Java: `ValidationProblem`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationProblem {
    pub field: &'static str,
    pub message: String,
}

impl ExpensesFacade {
    /// Roles come from `teams` on every call; "today" from `clock`; at most `limits.max_batch_size` drafts
    /// at a time.
    #[must_use]
    pub fn new(teams: TeamsFacade, clock: Arc<dyn Clock>, limits: &Limits) -> Self {
        Self::with(Arc::new(TeamsMembershipLookup(teams)), clock, limits.max_batch_size)
    }

    pub(crate) fn with(memberships: Arc<dyn MembershipLookup>, clock: Arc<dyn Clock>, max_batch_size: u32) -> Self {
        Self {
            memberships,
            clock,
            max_batch_size,
        }
    }

    /// Checks a would-be expense against the expense rules without creating it, so that batch and import can
    /// report one bad item or row without writing the good ones first. Java: `validate(NewExpense)`.
    #[must_use]
    pub fn validate(&self, expense: &NewExpense) -> Option<ValidationProblem> {
        expense
            .check(self.clock.as_ref())
            .err()
            .map(|invalid| ValidationProblem {
                field: invalid.field,
                message: invalid.message,
            })
    }

    /// [`ExpensesFacade::validate`] without an amount: for a row whose amount `Money` refuses as too large
    /// for the database, which Java's `Money` lets through to the insert. Java: `Expense.checkFields`.
    #[must_use]
    pub fn validate_fields(
        &self,
        incurred_on: chrono::NaiveDate,
        merchant: &str,
        note: Option<&str>,
    ) -> Option<ValidationProblem> {
        crate::domain::check_fields(incurred_on, merchant, note, self.clock.as_ref())
            .err()
            .map(|invalid| ValidationProblem {
                field: invalid.field,
                message: invalid.message,
            })
    }

    /// Creates the drafts with `claimant` as their claimant, on `connection`: in the caller's transaction, so
    /// that either all of them are created or none is. The only way other modules create expenses (Java's
    /// decision D4).
    ///
    /// # Errors
    ///
    /// 404 unless the claimant is a teammate; 400 for an empty list or one longer than the batch limit; a
    /// draft that breaks a rule; a database failure.
    pub async fn create_drafts(
        &self,
        connection: &mut PgConnection,
        team: TeamId,
        claimant: UserId,
        drafts: &[NewExpense],
    ) -> Result<Vec<ExpenseId>, ExpenseRefusal> {
        let limit = self.max_batch_size;
        if drafts.is_empty() || drafts.len() > usize::try_from(limit).unwrap_or(usize::MAX) {
            return Err(UseCaseError::invalid("items", format!("Between 1 and {limit} expenses at a time")).into());
        }
        access::actor(connection, self.memberships.as_ref(), team, claimant).await?;
        let now = self.clock.now();
        let mut ids = Vec::with_capacity(drafts.len());
        for draft in drafts {
            let expense = Expense::draft(team, claimant, draft, self.clock.as_ref())
                .map_err(|invalid| UseCaseError::from(ExpenseError::from(invalid)))?;
            persistence::insert(connection, &expense, now)
                .await
                .map_err(UseCaseError::from)?;
            ids.push(expense.id());
        }
        Ok(ids)
    }

    /// Passes if the caller is a teammate who may see the expense.
    ///
    /// # Errors
    ///
    /// Not a teammate, no such expense in the team, or a colleague's expense for a MEMBER: all 404.
    pub async fn require_can_view(
        &self,
        connection: &mut PgConnection,
        team: TeamId,
        expense: ExpenseId,
        caller: UserId,
    ) -> Result<(), ExpenseRefusal> {
        let actor = access::actor(connection, self.memberships.as_ref(), team, caller).await?;
        access::load(connection, actor, team, expense).await?;
        Ok(())
    }

    /// Passes if the caller may add or remove receipts: the claimant, while the expense is a draft.
    ///
    /// # Errors
    ///
    /// 404 as [`ExpensesFacade::require_can_view`], then 403 for someone else than the claimant, then 409
    /// `EXPENSE_NOT_DRAFT`.
    pub async fn require_can_change_receipts(
        &self,
        connection: &mut PgConnection,
        team: TeamId,
        expense: ExpenseId,
        caller: UserId,
    ) -> Result<(), ExpenseRefusal> {
        let actor = access::actor(connection, self.memberships.as_ref(), team, caller).await?;
        let found = access::load(connection, actor, team, expense).await?;
        found
            .value
            .require_can_change_receipts(actor)
            .map_err(|refused| ExpenseRefusal(refused.into()))
    }
}

/// The facade on the teams module's memberships, as the app wires it.
impl Default for ExpensesFacade {
    fn default() -> Self {
        Self::new(TeamsFacade, Arc::new(SystemClock), &Limits::default())
    }
}

/// The expenses module said no, or could not answer.
#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub struct ExpenseRefusal(#[from] pub(crate) UseCaseError);

impl ExpenseRefusal {
    /// The answer Java gives for it. A database failure is logged here and answered with a bare 500.
    #[must_use]
    pub fn problem(&self) -> Problem {
        failure(&self.0)
    }

    /// True for the 404 of an expense the caller may not see, which callers sometimes need to tell apart.
    #[must_use]
    pub const fn is_not_found(&self) -> bool {
        matches!(self.0, UseCaseError::NotFound)
    }
}
