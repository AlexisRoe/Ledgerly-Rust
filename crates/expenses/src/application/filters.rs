//! The filters of the expense list. The CSV export (M8) uses the same ones, so the two never disagree.
//!
//! Java counterpart: `expenses/internal/application/ExpenseFilters.java` and the `Specification`s of
//! `expenses/internal/infrastructure/ExpenseSpecifications.java`.

use chrono::NaiveDate;
use ledgerly_shared::id::{TeamId, UserId};
use ledgerly_shared::money::Currency;
use ledgerly_shared::role::Role;

use super::UseCaseError;
use crate::domain::{Actor, Category, ExpenseStatus};
use crate::persistence::Criteria;

/// Optional narrowing criteria as the client sent them; `None` means "do not filter on this".
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Filter {
    pub status: Option<ExpenseStatus>,
    pub category: Option<Category>,
    pub claimant: Option<UserId>,
    /// As sent: checked by [`currency_of`].
    pub currency: Option<String>,
    pub incurred_from: Option<NaiveDate>,
    pub incurred_to: Option<NaiveDate>,
}

/// The currency filter, if any, after checking it is a code `java.util.Currency.getInstance` accepts. That
/// includes codes without minor units (XXX), unlike [`Currency::from_code`]; no expense is in one, so they
/// select nothing.
pub(crate) fn currency_of(code: Option<&str>) -> Result<Option<String>, UseCaseError> {
    match code {
        None => Ok(None),
        Some(code) if Currency::is_known_code(code) => Ok(Some(code.to_owned())),
        Some(_) => Err(UseCaseError::invalid("currency", "Unknown currency code")),
    }
}

/// Scoped to the team; a MEMBER is forced to their own expenses whatever claimant they ask for.
pub(crate) fn build(actor: Actor, team: TeamId, filter: Filter) -> Result<Criteria, UseCaseError> {
    if let (Some(from), Some(to)) = (filter.incurred_from, filter.incurred_to)
        && from > to
    {
        return Err(UseCaseError::invalid(
            "incurredFrom",
            "incurredFrom must not be after incurredTo",
        ));
    }
    let currency = currency_of(filter.currency.as_deref())?;
    let claimant = if actor.role().includes(Role::Approver) {
        filter.claimant
    } else {
        Some(actor.id())
    };
    Ok(Criteria {
        team,
        claimant,
        status: filter.status,
        category: filter.category,
        currency,
        incurred_from: filter.incurred_from,
        incurred_to: filter.incurred_to,
    })
}
