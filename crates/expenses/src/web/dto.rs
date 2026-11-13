//! The JSON bodies of the expense endpoints, as `api/openapi.yaml` describes them.
//!
//! Java counterpart: the models generated into `com.ledgerly.shared.api.model` (`CreateExpenseRequest`,
//! `UpdateExpenseRequest`, `RejectRequest`, `Money`, `ExpenseResponse`) and `expenses/internal/web/ExpenseMapper`.
//!
//! Every request property is an `Option`, so that the Bean Validation rules of the generated Java models
//! (`@NotNull`, `@Size`, `@Pattern`) are checked in [`Validate`] and all broken ones are reported together.
//! Unknown properties are refused (`deny_unknown_fields`, Java's `fail-on-unknown-properties: true`), and there is
//! no `status`, `claimantId` or `version` to set (OWASP API3).

use std::str::FromStr;

use chrono::NaiveDate;
use ledgerly_shared::iso;
use ledgerly_shared::money::Money;
use ledgerly_shared::version::Versioned;
use ledgerly_shared::web::{Problem, Validate, Violations};
use serde::{Deserialize, Deserializer, Serialize};

use crate::application::ExpensePage;
use crate::domain::{Category, Expense};

const AMOUNT_PATTERN: &str = r"^[0-9]{1,15}(\.[0-9]{1,4})?$";
const CURRENCY_PATTERN: &str = "^[A-Z]{3}$";

/// `{"amount": "12.50", "currency": "EUR"}` as sent.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct MoneyBody {
    amount: Option<String>,
    currency: Option<String>,
}

impl MoneyBody {
    /// `@NotNull @Pattern` on both members, named `money.amount` and `money.currency` as Spring names nested
    /// fields.
    fn validate(&self, violations: &mut Violations) {
        violations.require("money.amount", self.amount.as_ref());
        violations.pattern("money.amount", self.amount.as_deref(), AMOUNT_PATTERN, is_amount);
        violations.require("money.currency", self.currency.as_ref());
        violations.pattern("money.currency", self.currency.as_deref(), CURRENCY_PATTERN, |code| {
            code.len() == 3 && code.bytes().all(|b| b.is_ascii_uppercase())
        });
    }

    /// The domain's `Money`, after validation. Java: `ExpenseMapper.toMoney`, whose failures are 400
    /// `VALIDATION_FAILED` for the field `money`, with `Money`'s own message.
    pub(crate) fn to_money(&self) -> Result<Money, Problem> {
        let amount = self.amount.as_deref().unwrap_or_default();
        let currency = self.currency.as_deref().unwrap_or_default();
        Money::parse(amount, currency).map_err(|error| Problem::invalid_request("money", error.to_string()))
    }
}

/// `^[0-9]{1,15}(\.[0-9]{1,4})?$`, matched against the whole value.
fn is_amount(amount: &str) -> bool {
    let (whole, fraction) = amount.split_once('.').map_or((amount, None), |(w, f)| (w, Some(f)));
    let digits = |text: &str, max: usize| (1..=max).contains(&text.len()) && text.bytes().all(|b| b.is_ascii_digit());
    digits(whole, 15) && fraction.is_none_or(|fraction| digits(fraction, 4))
}

/// A category name. Anything else is a body Jackson could not read: 400 `MALFORMED_REQUEST`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CategoryBody(pub Category);

impl<'de> Deserialize<'de> for CategoryBody {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let name = String::deserialize(deserializer)?;
        Category::from_str(&name).map(Self).map_err(serde::de::Error::custom)
    }
}

/// An ISO date, `yyyy-MM-dd` exactly, as Java's `LocalDate` reads it. Anything else is `MALFORMED_REQUEST`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DateBody(pub NaiveDate);

impl<'de> Deserialize<'de> for DateBody {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        iso::parse_date(&text)
            .map(Self)
            .ok_or_else(|| serde::de::Error::custom("not an ISO date"))
    }
}

/// `POST /teams/{teamId}/expenses`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct CreateExpenseRequest {
    pub money: Option<MoneyBody>,
    pub category: Option<CategoryBody>,
    pub incurred_on: Option<DateBody>,
    pub merchant: Option<String>,
    pub note: Option<String>,
}

impl Validate for CreateExpenseRequest {
    fn validate(&self, violations: &mut Violations) {
        violations.require("money", self.money.as_ref());
        if let Some(money) = &self.money {
            money.validate(violations);
        }
        violations.require("category", self.category.as_ref());
        violations.require("incurredOn", self.incurred_on.as_ref());
        violations.require("merchant", self.merchant.as_ref());
        violations.size("merchant", self.merchant.as_deref(), 1, 200);
        violations.size("note", self.note.as_deref(), 0, 2000);
    }
}

/// `PATCH /teams/{teamId}/expenses/{expenseId}`: absent members stay unchanged.
///
/// The spec's `minProperties: 1` is not checked: Java's generated model cannot express it, so `{}` is an edit
/// that changes nothing.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct UpdateExpenseRequest {
    pub money: Option<MoneyBody>,
    pub category: Option<CategoryBody>,
    pub incurred_on: Option<DateBody>,
    pub merchant: Option<String>,
    pub note: Option<String>,
}

impl Validate for UpdateExpenseRequest {
    fn validate(&self, violations: &mut Violations) {
        if let Some(money) = &self.money {
            money.validate(violations);
        }
        violations.size("merchant", self.merchant.as_deref(), 1, 200);
        violations.size("note", self.note.as_deref(), 0, 2000);
    }
}

/// `POST /teams/{teamId}/expenses/{expenseId}:reject`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RejectRequest {
    pub reason: Option<String>,
}

impl Validate for RejectRequest {
    fn validate(&self, violations: &mut Violations) {
        violations.require("reason", self.reason.as_ref());
        violations.size("reason", self.reason.as_deref(), 1, 1000);
    }
}

/// The expense as the API shows it. No version (that is the ETag), no timestamps.
///
/// `note` is written as `null` when there is none: Java's generated `ExpenseResponse` has no `@JsonInclude`, and
/// Jackson's default includes nulls.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ExpenseResponse {
    id: String,
    team_id: String,
    claimant_id: String,
    money: MoneyResponse,
    category: &'static str,
    incurred_on: String,
    merchant: String,
    note: Option<String>,
    status: &'static str,
    rejection_reasons: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
struct MoneyResponse {
    amount: String,
    currency: &'static str,
}

impl From<&Versioned<Expense>> for ExpenseResponse {
    fn from(versioned: &Versioned<Expense>) -> Self {
        let expense = &versioned.value;
        Self {
            id: expense.id().to_string(),
            team_id: expense.team_id().to_string(),
            claimant_id: expense.claimant_id().to_string(),
            money: MoneyResponse {
                amount: expense.money().amount().to_string(),
                currency: expense.money().currency().code(),
            },
            category: expense.category().as_str(),
            incurred_on: expense.incurred_on().format("%Y-%m-%d").to_string(),
            merchant: expense.merchant().as_str().to_owned(),
            note: expense.note().map(|note| note.as_str().to_owned()),
            status: expense.status().as_str(),
            rejection_reasons: expense
                .rejections()
                .iter()
                .map(|rejection| rejection.reason().as_str().to_owned())
                .collect(),
        }
    }
}

/// `GET /teams/{teamId}/expenses`: one page in the shape of Spring Data's `PagedModel`.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct ExpensePageResponse {
    content: Vec<ExpenseResponse>,
    page: PageMetadata,
}

/// Java: the generated `PageMetadata(size, number, totalElements, totalPages)`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PageMetadata {
    size: i32,
    number: i32,
    total_elements: i64,
    total_pages: i32,
}

impl From<&ExpensePage> for ExpensePageResponse {
    fn from(page: &ExpensePage) -> Self {
        Self {
            content: page.items.iter().map(ExpenseResponse::from).collect(),
            page: PageMetadata {
                size: page.size,
                number: page.page,
                total_elements: page.total_elements,
                total_pages: page.total_pages,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_amount_pattern_is_javas() {
        for good in ["1", "12.5", "12.3456", "123456789012345", "0.0001"] {
            assert!(is_amount(good), "{good}");
        }
        for bad in [
            "",
            ".5",
            "1.",
            "1.23456",
            "1234567890123456",
            "-1",
            "1e3",
            "1,5",
            "١٢",
            "12.50\n",
        ] {
            assert!(!is_amount(bad), "{bad}");
        }
    }
}
