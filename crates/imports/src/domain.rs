//! Reading an import's CSV as Ledgerly-Java does, line by line.
//!
//! Java counterpart: `UploadImport.inspect` and `requireHeader` (the checks at upload), Spring Batch's
//! `FlatFileItemReader` with `SimpleRecordSeparatorPolicy` and `DelimitedLineTokenizer(",")` (the job's reader,
//! `ImportJobConfig`), and `RowProcessor` (one line → one expense, or the line's error).
//!
//! The Spring Batch rules copied here, from `spring-batch-infrastructure` 6.0.6:
//! - lines end at `\n`, `\r` or `\r\n` (`BufferedReader.readLine`), and one line is one record, so a stray
//!   quote cannot swallow the next line, and line numbers stay true (the header is line 1);
//! - a line that starts with `#` is a comment: it is skipped without an error, but it is counted;
//! - a field is cut at each comma outside quotes; a field that, apart from surrounding blanks, is enclosed in
//!   quotes loses them and `""` inside becomes `"`; any other field is kept exactly as written.

use ledgerly_expenses::{Category, NewExpense};
use ledgerly_shared::iso;
use ledgerly_shared::money::{Currency, Money, MoneyError};
use ledgerly_shared::text::{is_blank, strip};

/// The required columns, then the optional `note`. Java: `UploadImport.REQUIRED_HEADER`.
const REQUIRED_HEADER: [&str; 5] = ["date", "merchant", "amount", "currency", "category"];

/// Where an import is: `Queued`, then `Running`, then `Completed` or `Failed`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportStatus {
    Queued,
    Running,
    Completed,
    Failed,
}

impl ImportStatus {
    /// The name as Java, the database and the API write it.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "QUEUED",
            Self::Running => "RUNNING",
            Self::Completed => "COMPLETED",
            Self::Failed => "FAILED",
        }
    }

    pub(crate) fn parse(text: &str) -> Option<Self> {
        [Self::Queued, Self::Running, Self::Completed, Self::Failed]
            .into_iter()
            .find(|status| status.as_str() == text)
    }
}

/// Why an uploaded file is refused before a job exists. Java: the exceptions `UploadImport.inspect` throws.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Refusal {
    /// 400 `VALIDATION_FAILED` on `file`.
    Invalid(String),
    /// 415 `UNSUPPORTED_MEDIA_TYPE`.
    Unsupported(&'static str),
}

/// The lines of a text as `BufferedReader.readLine` returns them, with the byte offset after each line's
/// terminator. A final line without a terminator is returned; an empty one after the last terminator is not.
fn lines(text: &str) -> Vec<(&str, usize)> {
    let bytes = text.as_bytes();
    let mut lines = Vec::new();
    let mut start = 0;
    let mut position = 0;
    while position < bytes.len() {
        match bytes.get(position) {
            Some(b'\n') => {
                lines.push((text.get(start..position).unwrap_or_default(), position + 1));
                position += 1;
                start = position;
            }
            Some(b'\r') => {
                let end = position;
                position += if bytes.get(position + 1) == Some(&b'\n') { 2 } else { 1 };
                lines.push((text.get(start..end).unwrap_or_default(), position));
                start = position;
            }
            _ => position += 1,
        }
    }
    if start < bytes.len() {
        lines.push((text.get(start..).unwrap_or_default(), bytes.len()));
    }
    lines
}

/// Java's `inspect`: UTF-8 text, the header, no NUL byte, 1 to `max_rows` non-blank data lines. Returns the
/// row count.
///
/// Java decodes strictly while it reads, so an invalid byte is noticed when the reader gets to the line that
/// holds it: the lines before it are checked first, and their problems win.
pub(crate) fn inspect(content: &[u8], max_rows: u32) -> Result<u32, Refusal> {
    let (valid, broken) = match std::str::from_utf8(content) {
        Ok(text) => (text, false),
        Err(error) => (
            std::str::from_utf8(content.get(..error.valid_up_to()).unwrap_or_default()).unwrap_or_default(),
            true,
        ),
    };
    let not_utf8 = || Refusal::Unsupported("The file must be UTF-8 text");
    let complete = lines(valid);
    // With a broken byte, the last line of the valid part is incomplete: reading it is what fails.
    let usable = if broken {
        complete
            .iter()
            .filter(|(_, end)| {
                *end <= valid.len()
                    && valid
                        .as_bytes()
                        .get(end - 1)
                        .is_some_and(|b| *b == b'\n' || *b == b'\r')
            })
            .count()
    } else {
        complete.len()
    };
    let mut lines = complete.into_iter().take(usable);
    let Some((header, _)) = lines.next() else {
        return Err(if broken {
            not_utf8()
        } else {
            Refusal::Invalid("The file is empty".to_owned())
        });
    };
    require_header(header)?;
    let mut rows: u32 = 0;
    for (line, _) in lines {
        if line.contains('\0') {
            return Err(Refusal::Unsupported("The file is not a CSV text file"));
        }
        if !is_blank(line) {
            rows += 1;
            if rows > max_rows {
                return Err(Refusal::Invalid(format!("At most {max_rows} rows per import")));
            }
        }
    }
    if broken {
        return Err(not_utf8());
    }
    if rows == 0 {
        return Err(Refusal::Invalid("The file has no data rows".to_owned()));
    }
    Ok(rows)
}

/// `date,merchant,amount,currency,category`, optionally `,note`, each column stripped and in any case; a
/// spreadsheet's byte order mark is allowed.
fn require_header(header: &str) -> Result<(), Refusal> {
    let header = header.strip_prefix('\u{feff}').unwrap_or(header);
    if header.contains('\0') {
        return Err(Refusal::Unsupported("The file is not a CSV text file"));
    }
    let columns: Vec<String> = header.split(',').map(|column| strip(column).to_lowercase()).collect();
    let required = REQUIRED_HEADER.len();
    let matches = (columns.len() == required
        || (columns.len() == required + 1 && columns.last().is_some_and(|c| c == "note")))
        && columns
            .iter()
            .zip(REQUIRED_HEADER)
            .all(|(column, expected)| column == expected);
    if matches {
        Ok(())
    } else {
        Err(Refusal::Invalid(
            "The first line must be: date,merchant,amount,currency,category[,note]".to_owned(),
        ))
    }
}

/// One record of the job's reader: its line number (the header is 1) and its fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Record {
    pub line: u32,
    pub fields: Vec<String>,
}

/// The data records of a file, as the job reads them: the header skipped, comment lines skipped but counted.
pub(crate) fn records(text: &str) -> Vec<Record> {
    lines(text)
        .into_iter()
        .zip(1_u32..)
        .skip(1)
        .filter(|((line, _), _)| !line.starts_with('#'))
        .map(|((line, _), number)| Record {
            line: number,
            fields: tokenize(line),
        })
        .collect()
}

/// `DelimitedLineTokenizer.doTokenize` with the delimiter `,` and the quote `"`.
pub(crate) fn tokenize(line: &str) -> Vec<String> {
    let chars: Vec<char> = line.chars().collect();
    let length = chars.len();
    let mut tokens = Vec::new();
    let mut in_quoted = false;
    let mut last_cut = 0_usize;
    for (i, &current) in chars.iter().enumerate() {
        let is_end = i + 1 == length;
        let is_delimiter = current == ',';
        if (is_delimiter && !in_quoted) || is_end {
            let mut end_position = if is_end { length - last_cut } else { i - last_cut };
            if is_end && is_delimiter {
                end_position -= 1;
            }
            tokens.push(unquoted(&chars, last_cut, end_position));
            if is_end && is_delimiter {
                tokens.push(String::new());
            }
            last_cut = i + 1;
        } else if current == '"' {
            in_quoted = !in_quoted;
        }
    }
    tokens
}

/// `substringWithTrimmedWhitespaceAndQuotesIfQuotesPresent`: blanks (`<= ' '`) around a quoted field are
/// dropped with the quotes, and `""` inside becomes `"`; an unquoted field is returned untouched.
fn unquoted(chars: &[char], offset: usize, count: usize) -> String {
    let mut start = offset;
    let mut len = count;
    while len > 1 && chars.get(start).is_some_and(|c| *c <= ' ') {
        start += 1;
        len -= 1;
    }
    while len > 0 && chars.get(start + len - 1).is_some_and(|c| *c <= ' ') {
        len -= 1;
    }
    let quoted = len >= 2 && chars.get(start) == Some(&'"') && chars.get(start + len - 1) == Some(&'"');
    if quoted {
        chars
            .get(start + 1..start + len - 1)
            .unwrap_or_default()
            .iter()
            .collect::<String>()
            .replace("\"\"", "\"")
    } else {
        chars.get(offset..offset + count).unwrap_or_default().iter().collect()
    }
}

/// What one record becomes. Java: `RowProcessor.process`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Row {
    /// An empty line, or one of blank fields: not a row, and not an error.
    Blank,
    /// The line's error, as the user is shown it.
    Rejected { code: &'static str, message: String },
    /// An expense to check against the expense rules and create.
    Ready(NewExpense),
}

/// The column checks of `RowProcessor`, in its order: column count, date, currency, amount, category. The
/// expense rules (merchant, note, a date in the future) come after, through the expenses module.
pub(crate) fn parse_row(fields: &[String]) -> Row {
    if fields.iter().all(|field| is_blank(field)) {
        return Row::Blank;
    }
    let rejected = |code, message: &str| Row::Rejected {
        code,
        message: message.to_owned(),
    };
    let ([date, merchant, amount, currency, category], note) = match fields {
        [a, b, c, d, e] => ([a, b, c, d, e], None),
        [a, b, c, d, e, f] => ([a, b, c, d, e], Some(f)),
        _ => {
            return rejected(
                "COLUMN_COUNT",
                &format!("Expected 5 or 6 columns but found {}", fields.len()),
            );
        }
    };
    let Some(incurred_on) = iso::parse_date(strip(date)) else {
        return rejected("DATE_INVALID", "date must look like 2026-11-05");
    };
    let code = strip(currency).to_uppercase();
    if !Currency::is_known_code(&code) {
        return rejected("CURRENCY_INVALID", "Unknown currency code");
    }
    let merchant = strip(merchant).to_owned();
    let note = note.filter(|note| !is_blank(note)).map(|note| strip(note).to_owned());
    // An amount over `numeric(19,4)` is `Money`'s `TooLarge`, a row error here; Java's `Money` lets it through
    // and the insert fails the import (the reviewer's decision after M8).
    let money = match Money::parse(strip(amount), &code) {
        Ok(money) => money,
        Err(MoneyError::NotADecimal) => return rejected("AMOUNT_INVALID", "amount is not a number"),
        Err(error) => return rejected("AMOUNT_INVALID", &error.to_string()),
    };
    let Ok(category) = strip(category).to_uppercase().parse::<Category>() else {
        return rejected(
            "CATEGORY_INVALID",
            "category must be one of TRAVEL, MEALS, OFFICE, SOFTWARE, OTHER",
        );
    };
    Row::Ready(NewExpense {
        money,
        category,
        incurred_on,
        merchant,
        note,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tokens(line: &str) -> Vec<String> {
        tokenize(line)
    }

    #[test]
    fn lines_end_where_buffered_reader_ends_them() {
        let found: Vec<&str> = lines("a\nb\r\nc\rd\n\ne").into_iter().map(|(line, _)| line).collect();
        assert_eq!(found, ["a", "b", "c", "d", "", "e"]);
        assert_eq!(lines("a\n").len(), 1);
        assert!(lines("").is_empty());
    }

    #[test]
    fn fields_are_cut_and_unquoted_as_spring_batch_does() {
        assert_eq!(tokens("a,b,c"), ["a", "b", "c"]);
        assert_eq!(tokens("a,b,"), ["a", "b", ""]);
        assert_eq!(tokens(r#"x,"Smith, Jones",y"#), ["x", "Smith, Jones", "y"]);
        assert_eq!(tokens(r#" "a ""q"" b" ,c"#), [r#"a "q" b"#, "c"]);
        // An unquoted field keeps its blanks.
        assert_eq!(tokens(" a , b"), [" a ", " b"]);
        // An unterminated quote runs to the end of the line, and a trailing comma still adds an empty field.
        assert_eq!(
            tokens(r#"2026-01-05,"unterminated,5.00,EUR,MEALS,"#),
            ["2026-01-05", r#""unterminated,5.00,EUR,MEALS"#, ""]
        );
        assert!(tokens("").is_empty());
        assert_eq!(tokens(" "), [" "]);
    }

    #[test]
    fn records_skip_the_header_and_comments_but_count_their_lines() {
        let found = records("date,merchant\n# a comment\n\nx,y\r\nz");
        let numbers: Vec<u32> = found.iter().map(|record| record.line).collect();
        assert_eq!(numbers, [3, 4, 5]);
        assert!(found[0].fields.is_empty());
    }

    #[test]
    fn the_upload_checks_and_their_messages() {
        let ok = "date,merchant,amount,currency,category,note\n2026-01-05,Cafe,5.00,EUR,MEALS,\n\n";
        assert_eq!(inspect(ok.as_bytes(), 10), Ok(1));
        assert_eq!(
            inspect("\u{feff} Date , MERCHANT,amount,currency,category\nx".as_bytes(), 10),
            Ok(1)
        );
        let invalid = |message: &str| Err(Refusal::Invalid(message.to_owned()));
        assert_eq!(inspect(b"", 10), invalid("The file is empty"));
        assert_eq!(
            inspect(b"date,merchant\nx", 10),
            invalid("The first line must be: date,merchant,amount,currency,category[,note]")
        );
        assert_eq!(
            inspect(b"date,merchant,amount,currency,category\n \n", 10),
            invalid("The file has no data rows")
        );
        assert_eq!(
            inspect(b"date,merchant,amount,currency,category\na\nb\nc\n", 2),
            invalid("At most 2 rows per import")
        );
        assert_eq!(
            inspect(b"date,merchant,amount,currency,category\na\0b\n", 10),
            Err(Refusal::Unsupported("The file is not a CSV text file"))
        );
        assert_eq!(
            inspect(b"date,merchant,amount,currency,category\nabc\xff\n", 10),
            Err(Refusal::Unsupported("The file must be UTF-8 text"))
        );
        assert_eq!(
            inspect(b"MZ\x90\0\x03\0\0\0\x04\0\0\0\xff\xff\0\0", 10),
            Err(Refusal::Unsupported("The file must be UTF-8 text"))
        );
        // A wrong header is found before an invalid byte further down.
        assert_eq!(
            inspect(b"date\nabc\xff\n", 10),
            invalid("The first line must be: date,merchant,amount,currency,category[,note]")
        );
    }

    fn row(line: &str) -> Row {
        parse_row(&tokenize(line))
    }

    fn code(line: &str) -> &'static str {
        match row(line) {
            Row::Rejected { code, .. } => code,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn rows_are_checked_in_javas_order_with_its_codes() {
        assert_eq!(code("2026-01-05,only,three"), "COLUMN_COUNT");
        assert_eq!(code("2026-01-05,Cafe,5.00,EUR,MEALS,note,extra"), "COLUMN_COUNT");
        assert_eq!(code("2026-13-45,Cafe,5.00,EUR,MEALS,"), "DATE_INVALID");
        assert_eq!(code("2026-01-05,Cafe,abc,ZZZ,MEALS,"), "CURRENCY_INVALID");
        assert_eq!(code("2026-01-05,Cafe,abc,EUR,FOOD,"), "AMOUNT_INVALID");
        assert_eq!(code("2026-01-05,Cafe,1.234,EUR,MEALS,"), "AMOUNT_INVALID");
        assert_eq!(code("2026-01-05,Cafe,5.00,EUR,FOOD,"), "CATEGORY_INVALID");
        assert_eq!(
            row("2026-01-05,Cafe,-5.00,EUR,MEALS,"),
            Row::Rejected {
                code: "AMOUNT_INVALID",
                message: "Amount must be positive: -5.00".to_owned()
            }
        );
        assert_eq!(row(""), Row::Blank);
        assert_eq!(row(" , ,"), Row::Blank);
    }

    #[test]
    fn values_are_stripped_and_codes_read_in_any_case() {
        let Row::Ready(expense) = row(" 2026-01-03 , Cafe ,5.00, eur ,travel,  a note ") else {
            panic!("not ready");
        };
        assert_eq!(expense.merchant, "Cafe");
        assert_eq!(expense.money.currency().code(), "EUR");
        assert_eq!(expense.category.as_str(), "TRAVEL");
        assert_eq!(expense.note.as_deref(), Some("a note"));
        let Row::Ready(without) = row("2026-01-03,Cafe,5.00,EUR,MEALS,  ") else {
            panic!("not ready");
        };
        assert_eq!(without.note, None);
        assert_eq!(
            row("2026-01-03,Cafe,12345678901234567.00,EUR,MEALS"),
            Row::Rejected {
                code: "AMOUNT_INVALID",
                message: "Amount 12345678901234567.00 has more than 15 digits before the decimal point".to_owned()
            }
        );
    }
}
