//! Java counterpart: `expenses/internal/application/UpdateExpense.java`.

use std::sync::Arc;

use chrono::NaiveDate;
use ledgerly_shared::clock::Clock;
use ledgerly_shared::events::Publisher;
use ledgerly_shared::id::{ExpenseId, TeamId, UserId};
use ledgerly_shared::money::Money;
use ledgerly_shared::text::is_blank;
use ledgerly_shared::version::{Version, Versioned};
use sqlx::PgPool;

use super::{MembershipLookup, UseCaseError, access};
use crate::domain::{Category, Expense, NewExpense};
use crate::events::{ExpenseEvent, ExpenseEventKind};
use crate::persistence;

/// A partial edit: `None` leaves a field as it is. A blank `note` clears the note.
#[derive(Debug, Clone)]
pub(crate) struct UpdateExpenseCommand {
    pub team: TeamId,
    pub id: ExpenseId,
    pub caller: UserId,
    pub expected: Version,
    pub money: Option<Money>,
    pub category: Option<Category>,
    pub incurred_on: Option<NaiveDate>,
    pub merchant: Option<String>,
    pub note: Option<String>,
}

/// Edits the fields of a draft, with optimistic locking.
#[derive(Debug, Clone)]
pub(crate) struct UpdateExpense {
    pool: PgPool,
    clock: Arc<dyn Clock>,
    memberships: Arc<dyn MembershipLookup>,
    events: Arc<dyn Publisher>,
}

impl UpdateExpense {
    pub(crate) fn new(
        pool: PgPool,
        clock: Arc<dyn Clock>,
        memberships: Arc<dyn MembershipLookup>,
        events: Arc<dyn Publisher>,
    ) -> Self {
        Self {
            pool,
            clock,
            memberships,
            events,
        }
    }

    /// Checks, in Java's order: teammate and visible (404), current version (412), then the domain: claimant
    /// (403), draft (409), field rules (400).
    ///
    /// An edit that changes nothing writes nothing: version and `updated_at` stay, as with Hibernate's dirty
    /// checking (decided after M2).
    ///
    /// # Errors
    ///
    /// [`UseCaseError`] for each of the refusals above, or a storage failure.
    pub(crate) async fn execute(&self, command: UpdateExpenseCommand) -> Result<Versioned<Expense>, UseCaseError> {
        let mut transaction = self.pool.begin().await?;
        let actor = access::actor(
            &mut transaction,
            self.memberships.as_ref(),
            command.team,
            command.caller,
        )
        .await?;
        let found = access::load(&mut transaction, actor, command.team, command.id).await?;
        access::require_version(&found, command.expected)?;
        let current = &found.value;
        let note = match command.note {
            None => current.note().map(|note| note.as_str().to_owned()),
            Some(note) if is_blank(&note) => None,
            Some(note) => Some(note),
        };
        let wanted = NewExpense {
            money: command.money.unwrap_or(current.money()),
            category: command.category.unwrap_or(current.category()),
            incurred_on: command.incurred_on.unwrap_or(current.incurred_on()),
            merchant: command
                .merchant
                .unwrap_or_else(|| current.merchant().as_str().to_owned()),
            note,
        };
        let changed_fields = changed_fields(current, &wanted);
        let mut edited = current.clone();
        edited.edit(actor, &wanted, self.clock.as_ref())?;
        // Java publishes on every successful edit, also when nothing changed (an empty list).
        let event = ExpenseEvent::new(
            command.team,
            command.id,
            command.caller,
            self.clock.now(),
            ExpenseEventKind::Edited { changed_fields },
        );
        self.events.publish(&mut transaction, &event).await?;
        if edited == found.value {
            transaction.commit().await?;
            return Ok(found);
        }
        let version = persistence::update(&mut transaction, &edited, found.version, self.clock.now()).await?;
        transaction.commit().await?;
        Ok(Versioned { value: edited, version })
    }
}

/// The names of the fields `wanted` changes, in Java's order. As in Java, the merchant and the note are compared
/// as the client sent them (a blank note being no note), before the domain tidies them.
fn changed_fields(current: &Expense, wanted: &NewExpense) -> Vec<String> {
    let mut changed = Vec::new();
    if wanted.money != current.money() {
        changed.push("money");
    }
    if wanted.category != current.category() {
        changed.push("category");
    }
    if wanted.incurred_on != current.incurred_on() {
        changed.push("incurredOn");
    }
    if wanted.merchant != current.merchant().as_str() {
        changed.push("merchant");
    }
    if wanted.note.as_deref() != current.note().map(|note| note.as_str()) {
        changed.push("note");
    }
    changed.into_iter().map(str::to_owned).collect()
}
