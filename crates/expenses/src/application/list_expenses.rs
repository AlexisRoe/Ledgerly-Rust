//! Java counterpart: `expenses/internal/application/ListExpenses.java`.

use std::sync::Arc;

use ledgerly_shared::id::{TeamId, UserId};
use ledgerly_shared::version::Versioned;
use sqlx::PgPool;

use super::filters::{self, Filter};
use super::{MembershipLookup, UseCaseError, access};
use crate::domain::Expense;
use crate::persistence::{self, Direction, Order, SortField};

/// One request for a page. Paging is zero-based; `sort` is `field[,asc|desc]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ListQuery {
    pub team: TeamId,
    pub caller: UserId,
    pub page: i32,
    pub size: i32,
    pub sort: String,
    pub filter: Filter,
}

/// One page plus the totals a client needs to build page links. Java: `ListExpenses.PageResult`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExpensePage {
    pub items: Vec<Versioned<Expense>>,
    pub page: i32,
    pub size: i32,
    pub total_elements: i64,
    pub total_pages: i32,
}

/// Lists the expenses the caller may see: all of the team's for approvers and owners, their own for members.
#[derive(Debug, Clone)]
pub(crate) struct ListExpenses {
    pool: PgPool,
    memberships: Arc<dyn MembershipLookup>,
    max_page_size: u32,
}

impl ListExpenses {
    pub(crate) fn new(pool: PgPool, memberships: Arc<dyn MembershipLookup>, max_page_size: u32) -> Self {
        Self {
            pool,
            memberships,
            max_page_size,
        }
    }

    /// Checks in Java's order: teammate (404), page, size, currency, sort, then the filters. Runs in a
    /// read-only transaction, as Java's `@Transactional(readOnly = true)` does, so the page and the count see
    /// the same rows.
    ///
    /// # Errors
    ///
    /// [`UseCaseError::NotFound`] for a non-teammate, [`UseCaseError::Invalid`] for a bad page, size, sort or
    /// filter, or a storage failure.
    pub(crate) async fn execute(&self, query: ListQuery) -> Result<ExpensePage, UseCaseError> {
        let mut transaction = self.pool.begin().await?;
        sqlx::query("set transaction isolation level repeatable read, read only")
            .execute(&mut *transaction)
            .await?;
        let actor = access::actor(&mut transaction, self.memberships.as_ref(), query.team, query.caller).await?;
        if query.page < 0 {
            return Err(UseCaseError::invalid("page", "page must not be negative"));
        }
        let size = u32::try_from(query.size)
            .ok()
            .filter(|size| (1..=self.max_page_size).contains(size))
            .ok_or_else(|| {
                UseCaseError::invalid("size", format!("size must be between 1 and {}", self.max_page_size))
            })?;
        let has_currency = filters::currency_of(query.filter.currency.as_deref())?.is_some();
        let order = parse_sort(&query.sort, has_currency)?;
        let criteria = filters::build(actor, query.team, query.filter)?;

        let offset = i64::from(query.page) * i64::from(size);
        let items = persistence::list(&mut transaction, &criteria, order, offset, size).await?;
        // Spring Data's `PageableExecutionUtils.getPage`: no count query when this page shows where the end is.
        let returned = i64::try_from(items.len()).unwrap_or(i64::MAX);
        let total_elements = if (offset == 0 || returned > 0) && returned < i64::from(size) {
            offset + returned
        } else {
            persistence::count(&mut transaction, &criteria).await?
        };
        transaction.commit().await?;
        let pages = (total_elements + i64::from(size) - 1) / i64::from(size);
        Ok(ExpensePage {
            items,
            page: query.page,
            size: query.size,
            total_elements,
            total_pages: i32::try_from(pages).unwrap_or(i32::MAX),
        })
    }
}

/// `field[,asc|desc]`, descending by default, with the id as a unique tie-breaker in the same direction so that
/// pages never overlap. Java: `ListExpenses.parseSort` and `SortField`.
fn parse_sort(raw: &str, has_currency_filter: bool) -> Result<Order, UseCaseError> {
    let parts: Vec<&str> = raw.split(',').collect();
    let (name, direction) = match parts.as_slice() {
        [name] => (*name, None),
        [name, direction] => (*name, Some(*direction)),
        _ => return Err(UseCaseError::invalid("sort", "Use <field>,<asc|desc>")),
    };
    let field = match name {
        "incurredOn" => SortField::IncurredOn,
        "createdAt" => SortField::CreatedAt,
        "amount" => SortField::Amount,
        _ => {
            return Err(UseCaseError::invalid(
                "sort",
                "Sort field must be one of incurredOn, createdAt, amount",
            ));
        }
    };
    let direction = match direction {
        None | Some("desc") => Direction::Descending,
        Some("asc") => Direction::Ascending,
        Some(_) => return Err(UseCaseError::invalid("sort", "Direction must be asc or desc")),
    };
    if field == SortField::Amount && !has_currency_filter {
        return Err(UseCaseError::invalid(
            "sort",
            "Sorting by amount needs a currency filter",
        ));
    }
    Ok(Order { field, direction })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(result: Result<Order, UseCaseError>) -> String {
        match result {
            Err(UseCaseError::Invalid { field, message }) => format!("{field}: {message}"),
            other => panic!("expected a refusal, got {other:?}"),
        }
    }

    #[test]
    fn sort_defaults_to_descending_and_reads_the_three_fields() {
        let order = |field, direction| Some(Order { field, direction });
        assert_eq!(
            parse_sort("incurredOn", false).ok(),
            order(SortField::IncurredOn, Direction::Descending)
        );
        assert_eq!(
            parse_sort("createdAt,asc", false).ok(),
            order(SortField::CreatedAt, Direction::Ascending)
        );
        assert_eq!(
            parse_sort("amount,desc", true).ok(),
            order(SortField::Amount, Direction::Descending)
        );
    }

    #[test]
    fn sort_refusals_carry_javas_messages() {
        assert_eq!(message(parse_sort("a,b,c", true)), "sort: Use <field>,<asc|desc>");
        for name in ["password", "id.value", "money.amount", "", "IncurredOn"] {
            assert_eq!(
                message(parse_sort(name, true)),
                "sort: Sort field must be one of incurredOn, createdAt, amount",
                "{name}"
            );
        }
        assert_eq!(
            message(parse_sort("incurredOn,sideways", true)),
            "sort: Direction must be asc or desc"
        );
        assert_eq!(
            message(parse_sort("incurredOn,", true)),
            "sort: Direction must be asc or desc"
        );
        assert_eq!(
            message(parse_sort("amount", false)),
            "sort: Sorting by amount needs a currency filter"
        );
    }
}
