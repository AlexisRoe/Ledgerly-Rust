//! The CSV export's cells and rows.
//!
//! Java counterpart: `expenses/internal/web/CsvCells.java` and `LazyCsvInputStream.java`. The security point is
//! [`neutralise`]: a spreadsheet treats a cell that starts with `=`, `+`, `-`, `@`, a tab or a carriage return
//! as a formula, so a merchant called `=HYPERLINK(...)` could run in an approver's spreadsheet. Such cells get
//! a leading quote and show as text.

use ledgerly_shared::version::Versioned;

use crate::domain::Expense;

/// The header row. Java: `LazyCsvInputStream.HEADER`.
pub(crate) const HEADER: &str = "id,incurredOn,merchant,amount,currency,category,status,claimantId,note\r\n";

/// One field: neutralised, then quoted if it holds a comma, a quote or a line break.
pub(crate) fn cell(value: &str) -> String {
    let text = neutralise(value);
    if text.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", text.replace('"', "\"\""))
    } else {
        text
    }
}

/// A leading quote for a value that starts like a formula.
pub(crate) fn neutralise(value: &str) -> String {
    match value.chars().next() {
        Some('=' | '+' | '-' | '@' | '\t' | '\r') => format!("'{value}"),
        _ => value.to_owned(),
    }
}

/// One CRLF-terminated line.
pub(crate) fn row(cells: &[&str]) -> String {
    let mut line = cells.iter().map(|value| cell(value)).collect::<Vec<_>>().join(",");
    line.push_str("\r\n");
    line
}

/// The lines of a page of expenses, in Java's column order; the amount as `toPlainString()` writes it.
pub(crate) fn rows(page: &[Versioned<Expense>]) -> String {
    page.iter()
        .map(|versioned| {
            let expense = &versioned.value;
            let money = expense.money();
            row(&[
                &expense.id().to_string(),
                &expense.incurred_on().to_string(),
                expense.merchant().as_str(),
                &money.amount().to_string(),
                money.currency().code(),
                expense.category().as_str(),
                expense.status().as_str(),
                &expense.claimant_id().to_string(),
                expense.note().map_or("", |note| note.as_str()),
            ])
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Java: `CsvCellsTest`.
    #[test]
    fn formula_starts_get_a_quote_and_specials_are_quoted() {
        for (value, expected) in [
            ("Cafe", "Cafe"),
            ("=1+1", "'=1+1"),
            ("+1", "'+1"),
            ("-2+3", "'-2+3"),
            ("@SUM(A1)", "'@SUM(A1)"),
            ("\tTabbed", "'\tTabbed"),
            ("\rx", "\"'\rx\""),
            ("Smith, Jones & Co", "\"Smith, Jones & Co\""),
            (r#"say "hi""#, r#""say ""hi""""#),
            (
                r#"=HYPERLINK("http://evil.test","click")"#,
                r#""'=HYPERLINK(""http://evil.test"",""click"")""#,
            ),
            ("", ""),
            ("a-b", "a-b"),
        ] {
            assert_eq!(cell(value), expected, "{value:?}");
        }
    }

    #[test]
    fn the_header_is_javas() {
        assert_eq!(
            HEADER,
            row(&[
                "id",
                "incurredOn",
                "merchant",
                "amount",
                "currency",
                "category",
                "status",
                "claimantId",
                "note"
            ])
        );
    }
}
