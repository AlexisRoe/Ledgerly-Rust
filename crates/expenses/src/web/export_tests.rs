//! The export's body, page by page, without a database. Java:
//! `src/test/java/com/ledgerly/expenses/internal/web/LazyCsvInputStreamTest.java`.
//!
//! Two of Java's tests have no counterpart: `readingByteByByteAndInBulkGiveTheSameBytes` and
//! `aZeroLengthReadReturnsZeroWithoutConsumingAnything` test `InputStream.read`'s two forms. The Rust body is a
//! stream of whole chunks that hyper writes; there is no byte-wise read to compare.

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use chrono::{NaiveDate, TimeZone, Utc};
use futures_util::StreamExt;
use ledgerly_shared::clock::FixedClock;
use ledgerly_shared::id::{TeamId, UserId};
use ledgerly_shared::money::Money;
use ledgerly_shared::version::{Version, Versioned};

use super::{PageSource, csv, csv_stream};
use crate::application::UseCaseError;
use crate::domain::{Category, Expense, NewExpense};

/// Hands out the pages it was given, then empty ones, and counts how often it was asked.
struct Pages {
    pages: VecDeque<Vec<Versioned<Expense>>>,
    asked: Arc<AtomicUsize>,
}

impl Pages {
    fn of(pages: Vec<Vec<Versioned<Expense>>>) -> (Self, Arc<AtomicUsize>) {
        let asked = Arc::new(AtomicUsize::new(0));
        (
            Self {
                pages: pages.into(),
                asked: asked.clone(),
            },
            asked,
        )
    }
}

impl PageSource for Pages {
    async fn next_page(&mut self) -> Result<Vec<Versioned<Expense>>, UseCaseError> {
        self.asked.fetch_add(1, Ordering::SeqCst);
        Ok(self.pages.pop_front().unwrap_or_default())
    }
}

fn expense(merchant: &str, note: Option<&str>) -> Versioned<Expense> {
    let clock = FixedClock::at(Utc.with_ymd_and_hms(2026, 11, 17, 9, 0, 0).unwrap());
    let new = NewExpense {
        money: Money::parse("12.50", "EUR").unwrap(),
        category: Category::Meals,
        incurred_on: NaiveDate::from_ymd_opt(2026, 11, 5).unwrap(),
        merchant: merchant.to_owned(),
        note: note.map(str::to_owned),
    };
    Versioned {
        value: Expense::draft(TeamId::new_v7(), UserId::new_v7(), &new, &clock).unwrap(),
        version: Version::INITIAL,
    }
}

async fn read_all(pages: Pages) -> String {
    csv_stream(pages)
        .map(|chunk| chunk.unwrap())
        .collect::<Vec<_>>()
        .await
        .concat()
}

#[tokio::test]
async fn an_export_without_rows_is_just_the_header_line() {
    let (pages, _) = Pages::of(vec![]);

    let text = read_all(pages).await;

    assert_eq!(text, csv::HEADER);
    assert_eq!(text.lines().count(), 1);
}

#[tokio::test]
async fn nothing_is_fetched_before_the_header_has_been_read() {
    let (pages, asked) = Pages::of(vec![vec![expense("First", None)]]);
    let mut stream = Box::pin(csv_stream(pages));

    let header = stream.next().await.unwrap().unwrap();

    assert_eq!(header, csv::HEADER);
    assert_eq!(asked.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn rows_of_all_pages_come_in_order_and_an_empty_page_ends_the_stream_for_good() {
    let (pages, asked) = Pages::of(vec![
        vec![expense("First", None)],
        vec![expense("Second", None)],
        vec![],
        vec![expense("Never", None)],
    ]);

    let text = read_all(pages).await;

    assert_eq!(text.lines().count(), 3);
    assert!(text.find("First") < text.find("Second"));
    assert!(!text.contains("Never"));
    assert_eq!(asked.load(Ordering::SeqCst), 3, "nothing is asked after the empty page");
}

#[tokio::test]
async fn a_merchant_that_looks_like_a_formula_is_never_emitted_as_one() {
    let (pages, _) = Pages::of(vec![vec![
        expense(r#"=HYPERLINK("http://evil.test","x")"#, None),
        expense("+cmd", None),
    ]]);

    let text = read_all(pages).await;

    assert!(!text.contains(",=") && !text.contains(",+"), "{text}");
    assert!(text.contains("'=HYPERLINK") && text.contains("'+cmd"), "{text}");
}

#[tokio::test]
async fn a_note_with_commas_quotes_and_line_breaks_stays_one_cell() {
    let (pages, _) = Pages::of(vec![vec![expense("Cafe", Some("he said \"no\", twice\nthen left"))]]);

    let text = read_all(pages).await;

    assert!(text.contains("\"he said \"\"no\"\", twice\nthen left\""), "{text}");
}

#[tokio::test]
async fn a_missing_note_is_an_empty_last_cell() {
    let (pages, _) = Pages::of(vec![vec![expense("Cafe", None)]]);

    let text = read_all(pages).await;

    assert!(text.lines().nth(1).unwrap().ends_with(','), "{text}");
}

#[tokio::test]
async fn the_row_count_matches_what_was_handed_out() {
    let page = |count| (0..count).map(|_| expense("Cafe", None)).collect::<Vec<_>>();
    let (pages, _) = Pages::of(vec![page(500), page(500), page(200)]);

    assert_eq!(read_all(pages).await.lines().count(), 1201);
}
