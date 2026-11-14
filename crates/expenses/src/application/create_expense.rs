//! Java counterpart: `expenses/internal/application/CreateExpense.java`.

use std::sync::Arc;

use ledgerly_shared::clock::Clock;
use ledgerly_shared::id::{ExpenseId, TeamId, UserId};
use ledgerly_shared::idempotency::{self, Entry, SaveError};
use ledgerly_shared::version::Versioned;
use sqlx::PgPool;

use super::{MembershipLookup, UseCaseError, access};
use crate::domain::{Expense, ExpenseError, NewExpense};
use crate::persistence;

/// What the caller asks for. The claimant is always the caller; nothing in here can set a status or a version.
#[derive(Debug, Clone)]
pub(crate) struct CreateExpenseCommand {
    pub team: TeamId,
    pub claimant: UserId,
    pub expense: NewExpense,
}

impl CreateExpenseCommand {
    /// The request's canonical form for the `Idempotency-Key` check: every field as received, the amount at the
    /// currency's scale. Not Java's `Command.toString()` (ADR 0005, accepted exceptions).
    fn hash(&self) -> String {
        let NewExpense {
            money,
            category,
            incurred_on,
            merchant,
            note,
        } = &self.expense;
        let note = note
            .as_deref()
            .map_or_else(|| "absent".to_owned(), |note| format!("note:{note}"));
        idempotency::request_hash(&[
            "create-expense/1",
            &self.team.to_string(),
            &self.claimant.to_string(),
            &money.amount().to_string(),
            money.currency().code(),
            category.as_str(),
            &incurred_on.to_string(),
            merchant,
            &note,
        ])
    }
}

/// Creates a draft expense for the caller.
#[derive(Debug, Clone)]
pub(crate) struct CreateExpense {
    pool: PgPool,
    clock: Arc<dyn Clock>,
    memberships: Arc<dyn MembershipLookup>,
}

impl CreateExpense {
    pub(crate) fn new(pool: PgPool, clock: Arc<dyn Clock>, memberships: Arc<dyn MembershipLookup>) -> Self {
        Self {
            pool,
            clock,
            memberships,
        }
    }

    /// Checks that the caller is a teammate, then the `Idempotency-Key` if there is one, then the fields, then
    /// stores the new draft at version 0 and the key, in one transaction.
    ///
    /// With a key that was used before (by this user, within 24 hours): the same request gets the expense as it
    /// is now; a different one is refused.
    ///
    /// # Errors
    ///
    /// [`UseCaseError::NotFound`] for a non-teammate; [`UseCaseError::IdempotencyKeyReused`] or
    /// [`UseCaseError::IdempotencyInProgress`]; [`UseCaseError::Refused`] with [`ExpenseError::InvalidInput`]
    /// naming the first field that breaks a rule (merchant, note, date in that order), before anything is
    /// written; [`UseCaseError::Storage`] if the database fails.
    pub(crate) async fn execute(
        &self,
        command: CreateExpenseCommand,
        idempotency_key: Option<&str>,
    ) -> Result<Versioned<Expense>, UseCaseError> {
        let mut transaction = self.pool.begin().await?;
        access::actor(
            &mut transaction,
            self.memberships.as_ref(),
            command.team,
            command.claimant,
        )
        .await?;
        let hash = command.hash();
        if let Some(key) = idempotency_key
            && let Some(previous) = idempotency::find(&mut transaction, command.claimant, key, self.clock.now()).await?
        {
            if previous.request_hash != hash {
                return Err(UseCaseError::IdempotencyKeyReused);
            }
            let id = ExpenseId::from_uuid(previous.resource_id);
            let current = persistence::find(&mut transaction, command.team, id).await?.ok_or(
                UseCaseError::IdempotencyInProgress("The expense created by this key no longer exists"),
            )?;
            transaction.commit().await?;
            return Ok(current);
        }
        let expense = Expense::draft(command.team, command.claimant, &command.expense, self.clock.as_ref())
            .map_err(ExpenseError::from)?;
        let version = persistence::insert(&mut transaction, &expense, self.clock.now()).await?;
        if let Some(key) = idempotency_key {
            let entry = Entry {
                request_hash: hash,
                response_status: 201,
                resource_id: expense.id().as_uuid(),
            };
            idempotency::save(&mut transaction, command.claimant, key, &entry, self.clock.now())
                .await
                .map_err(|error| match error {
                    SaveError::Taken => {
                        UseCaseError::IdempotencyInProgress("A request with this Idempotency-Key is in progress")
                    }
                    SaveError::Database(error) => UseCaseError::from(error),
                })?;
        }
        transaction.commit().await?;
        Ok(Versioned {
            value: expense,
            version,
        })
    }
}
