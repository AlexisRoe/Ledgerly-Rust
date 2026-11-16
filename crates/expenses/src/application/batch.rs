//! Java counterpart: `expenses/internal/application/BatchCreateExpenses.java`.

use std::sync::Arc;

use chrono::NaiveDate;
use ledgerly_shared::clock::Clock;
use ledgerly_shared::id::{ExpenseId, TeamId, UserId};
use ledgerly_shared::web::{ErrorCode, FieldViolation};
use sqlx::PgPool;

use super::{MembershipLookup, UseCaseError, access};
use crate::domain::{NewExpense, check_fields};
use crate::facade::ExpensesFacade;

/// One item of the request, parsed as far as the web layer could.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Candidate {
    /// Ready for the expense rules.
    Ready(NewExpense),
    /// An amount `Money` refuses as larger than the `numeric(19,4)` column. Java's `Money` has no such limit:
    /// the item passes the checks, and the insert fails in the database, which fails the whole request with
    /// a 500 (learning log M8). The fields are still checked, so a bad merchant is still that item's problem.
    TooLarge {
        incurred_on: NaiveDate,
        merchant: String,
        note: Option<String>,
    },
    /// The item could not be turned into an expense at all, for example an amount that is not a number.
    Unparseable { field: &'static str, message: String },
}

/// Why an item was refused: a stable code, the field if there is one, a readable message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ItemProblem {
    pub code: &'static str,
    pub field: Option<&'static str>,
    pub message: String,
}

/// The outcome of the item at `index`: the new expense's id, or its problem.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ItemResult {
    pub index: usize,
    pub outcome: Result<ExpenseId, ItemProblem>,
}

/// Creates up to 100 drafts in one call. Every item is checked first; then the valid ones are created
/// (partial success, the default), or with `atomic` one bad item refuses the whole batch. The writing goes
/// through [`ExpensesFacade::create_drafts`], the entry point imports use too.
#[derive(Debug, Clone)]
pub(crate) struct BatchCreateExpenses {
    pool: PgPool,
    clock: Arc<dyn Clock>,
    memberships: Arc<dyn MembershipLookup>,
    facade: ExpensesFacade,
    max_batch_size: u32,
}

impl BatchCreateExpenses {
    pub(crate) fn new(
        pool: PgPool,
        clock: Arc<dyn Clock>,
        memberships: Arc<dyn MembershipLookup>,
        max_batch_size: u32,
    ) -> Self {
        let facade = ExpensesFacade::with(memberships.clone(), clock.clone(), max_batch_size);
        Self {
            pool,
            clock,
            memberships,
            facade,
            max_batch_size,
        }
    }

    /// One result per item, in the order of the items.
    ///
    /// # Errors
    ///
    /// Not a teammate (404) before anything else; no items or more than the batch limit (400); with `atomic`,
    /// any bad item (422 `BATCH_REJECTED`, one entry per problem); an amount too large for the database (500).
    pub(crate) async fn execute(
        &self,
        team: TeamId,
        caller: UserId,
        items: Vec<Candidate>,
        atomic: bool,
    ) -> Result<Vec<ItemResult>, UseCaseError> {
        let mut transaction = self.pool.begin().await?;
        access::actor(&mut transaction, self.memberships.as_ref(), team, caller).await?;
        let limit = self.max_batch_size;
        if items.is_empty() || items.len() > usize::try_from(limit).unwrap_or(usize::MAX) {
            return Err(UseCaseError::invalid(
                "items",
                format!("Between 1 and {limit} items per batch"),
            ));
        }

        // Every item is checked; nothing is written yet.
        let problems: Vec<Option<ItemProblem>> = items.iter().map(|item| self.check(item)).collect();
        if atomic && problems.iter().any(Option::is_some) {
            let violations = problems
                .iter()
                .enumerate()
                .filter_map(|(index, problem)| {
                    problem.as_ref().map(|problem| {
                        let field = problem
                            .field
                            .map_or_else(|| format!("items[{index}]"), |field| format!("items[{index}].{field}"));
                        FieldViolation::new(field, problem.message.clone())
                    })
                })
                .collect();
            return Err(UseCaseError::BatchRejected(violations));
        }

        let mut valid = Vec::new();
        for (item, problem) in items.iter().zip(&problems) {
            match (item, problem) {
                (Candidate::Ready(expense), None) => valid.push(expense.clone()),
                // Java inserts it, and the database refuses the amount: the whole transaction fails.
                (Candidate::TooLarge { .. }, None) => return Err(UseCaseError::AmountOverflow),
                _ => {}
            }
        }
        let mut ids = if valid.is_empty() {
            Vec::new()
        } else {
            self.facade
                .create_drafts(&mut transaction, team, caller, &valid)
                .await
                .map_err(|refusal| refusal.0)?
        }
        .into_iter();
        transaction.commit().await?;

        let mut results = Vec::with_capacity(items.len());
        for (index, problem) in problems.into_iter().enumerate() {
            let outcome = match problem {
                Some(problem) => Err(problem),
                None => Ok(ids.next().ok_or(UseCaseError::AmountOverflow)?),
            };
            results.push(ItemResult { index, outcome });
        }
        Ok(results)
    }

    fn check(&self, item: &Candidate) -> Option<ItemProblem> {
        let problem = |field, message| ItemProblem {
            code: ErrorCode::ValidationFailed.code(),
            field: Some(field),
            message,
        };
        match item {
            Candidate::Unparseable { field, message } => Some(problem(field, message.clone())),
            Candidate::Ready(expense) => self
                .facade
                .validate(expense)
                .map(|invalid| problem(invalid.field, invalid.message)),
            Candidate::TooLarge {
                incurred_on,
                merchant,
                note,
            } => check_fields(*incurred_on, merchant, note.as_deref(), self.clock.as_ref())
                .err()
                .map(|invalid| problem(invalid.field, invalid.message)),
        }
    }
}
