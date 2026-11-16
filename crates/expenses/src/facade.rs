//! The expenses module's API for modules that hang data off an expense (receipts now, imports in M8).
//!
//! Java counterpart: `expenses/ExpensesFacade.java` and `internal/application/ExpensesFacadeImpl.java`. Both
//! checks fail with the usual API answers instead of returning flags, so a caller cannot forget to check. They
//! run on the caller's connection, so that the check and what the caller does next can share a transaction.

use std::sync::Arc;

use ledgerly_shared::id::{ExpenseId, TeamId, UserId};
use ledgerly_shared::web::Problem;
use ledgerly_teams::TeamsFacade;
use sqlx::PgConnection;

use crate::application::{MembershipLookup, TeamsMembershipLookup, UseCaseError, access};
use crate::web::problem::failure;

/// Who may see an expense, and who may change its receipts.
#[derive(Debug, Clone)]
pub struct ExpensesFacade {
    memberships: Arc<dyn MembershipLookup>,
}

impl ExpensesFacade {
    /// Roles come from `teams` on every call.
    #[must_use]
    pub fn new(teams: TeamsFacade) -> Self {
        Self {
            memberships: Arc::new(TeamsMembershipLookup(teams)),
        }
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
        Self::new(TeamsFacade)
    }
}

/// The expenses module said no, or could not answer.
#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub struct ExpenseRefusal(#[from] UseCaseError);

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
