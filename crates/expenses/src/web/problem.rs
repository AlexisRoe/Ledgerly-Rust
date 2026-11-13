//! How every refusal of the expenses module becomes a problem response.
//!
//! Java counterpart: `expenses/internal/web/ExpenseDomainExceptionHandler.java` for the domain's refusals, and
//! `ProblemExceptionHandler.lostRace`/`unexpected` for the rest. Java's `switch` over the sealed
//! `ExpenseException` is exhaustive; so are these `match`es, over [`ExpenseError`] and [`UseCaseError`]: a new
//! variant does not compile until it has a status.

use ledgerly_shared::web::{ErrorCode, Problem};

use crate::application::UseCaseError;
use crate::domain::ExpenseError;
use crate::persistence::PersistenceError;

/// The problem for a domain refusal. The details are the domain's messages, which are Java's.
pub(crate) fn refusal(error: &ExpenseError) -> Problem {
    match error {
        ExpenseError::InvalidTransition { .. } => Problem::new(ErrorCode::ExpenseInvalidTransition, error.to_string()),
        ExpenseError::SelfDealing { .. } => Problem::new(ErrorCode::ExpenseSelfDealing, error.to_string()),
        ExpenseError::NotDraft { .. } => Problem::new(ErrorCode::ExpenseNotDraft, error.to_string()),
        ExpenseError::Forbidden(forbidden) => Problem::forbidden(forbidden.to_string()),
        ExpenseError::RuleViolated(rule) => Problem::new(ErrorCode::ExpenseRuleViolated, *rule),
        ExpenseError::InvalidInput(invalid) => Problem::invalid_request(invalid.field, invalid.message.clone()),
    }
}

/// The problem for any failed use case.
///
/// A lost optimistic-lock race is 412, as in Java: the client re-reads. A database failure or a corrupt row is
/// logged and answered with a bare 500; neither the log line nor the body carries a value of the expense.
pub(crate) fn failure(error: &UseCaseError) -> Problem {
    match error {
        UseCaseError::NotFound => Problem::not_found(),
        UseCaseError::Stale => Problem::precondition_failed(),
        UseCaseError::Refused(refused) => refusal(refused),
        UseCaseError::Storage(PersistenceError::Conflict { .. }) => Problem::precondition_failed(),
        UseCaseError::Storage(storage @ (PersistenceError::Corrupt { .. } | PersistenceError::Database(_))) => {
            tracing::error!(error = %storage, "Unhandled exception");
            Problem::internal_error()
        }
    }
}

#[cfg(test)]
mod tests {
    use axum::http::StatusCode;
    use ledgerly_shared::role::Role;
    use ledgerly_shared::version::Version;
    use ledgerly_shared::web::FieldViolation;

    use super::*;
    use crate::domain::{Action, ExpenseStatus, Forbidden, InvalidInput};

    fn summary(problem: &Problem) -> (StatusCode, &'static str, Option<&str>) {
        (problem.status(), problem.code().code(), problem.detail())
    }

    /// Java: `ExpenseDomainExceptionHandler.handle`, one case per subclass of `ExpenseException`.
    #[test]
    fn every_domain_refusal_has_javas_status_code_and_message() {
        let cases = [
            (
                ExpenseError::InvalidTransition {
                    from: ExpenseStatus::Draft,
                    action: Action::Approve,
                },
                (
                    StatusCode::CONFLICT,
                    "EXPENSE_INVALID_TRANSITION",
                    "Expense is DRAFT and cannot APPROVE",
                ),
            ),
            (
                ExpenseError::SelfDealing {
                    action: Action::Approve,
                },
                (
                    StatusCode::CONFLICT,
                    "EXPENSE_SELF_DEALING",
                    "Claimants cannot APPROVE their own expense",
                ),
            ),
            (
                ExpenseError::NotDraft {
                    status: ExpenseStatus::Submitted,
                },
                (
                    StatusCode::CONFLICT,
                    "EXPENSE_NOT_DRAFT",
                    "Only a draft allows this, but the expense is SUBMITTED",
                ),
            ),
            (
                ExpenseError::Forbidden(Forbidden::RoleTooLow {
                    role: Role::Member,
                    action: Action::Approve,
                }),
                (StatusCode::FORBIDDEN, "FORBIDDEN", "Role MEMBER may not APPROVE"),
            ),
            (
                ExpenseError::RuleViolated("Receipts are still being scanned"),
                (
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "EXPENSE_RULE_VIOLATED",
                    "Receipts are still being scanned",
                ),
            ),
        ];
        for (error, (status, code, detail)) in cases {
            assert_eq!(summary(&refusal(&error)), (status, code, Some(detail)), "{error:?}");
        }
    }

    #[test]
    fn a_field_rule_is_a_validation_problem_naming_the_field() {
        let problem = refusal(&ExpenseError::InvalidInput(InvalidInput::new(
            "incurredOn",
            "incurredOn must not be in the future",
        )));

        assert_eq!(
            summary(&problem),
            (
                StatusCode::BAD_REQUEST,
                "VALIDATION_FAILED",
                Some("Request validation failed")
            )
        );
        assert_eq!(
            problem.violations(),
            [FieldViolation::new(
                "incurredOn",
                "incurredOn must not be in the future"
            )]
        );
    }

    #[test]
    fn a_lost_race_is_412_and_a_database_failure_a_bare_500() {
        let lost = failure(&UseCaseError::Storage(PersistenceError::Conflict {
            expected: Version::INITIAL,
        }));
        assert_eq!(summary(&lost).1, "PRECONDITION_FAILED");

        let broken = failure(&UseCaseError::Storage(PersistenceError::Database(
            sqlx::Error::PoolTimedOut,
        )));
        assert_eq!(
            summary(&broken),
            (StatusCode::INTERNAL_SERVER_ERROR, "INTERNAL_ERROR", None)
        );
    }
}
