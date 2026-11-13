//! CSV imports that create many drafts at once: parsing, row errors, limits, and who sees an import.
//!
//! Java: `src/test/java/com/ledgerly/imports/internal/web/ImportApiTest.java`. Its `importer`, `owner`,
//! `other` and `approver` are the team's MEMBER, OWNER, colleague and APPROVER here.
//!
//! Not here (see `NOT_PARITY.md`): the restart tests, which make the expenses facade fail in-process; the
//! redelivered event; and the daily allowance coming back the next day, which needs a clock. Java also checks
//! the `ImportCompleted` event it captured; from outside, the import's final state says the same.

// clippy.toml exempts `#[test]` functions only; the helpers of a test file are test code too.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

use std::collections::BTreeSet;
use std::time::Duration;

use ledgerly_parity_tests::fixtures::EXE;
use ledgerly_parity_tests::http::{Client, Response};
use ledgerly_parity_tests::{SignedInTeam, V1, World, eventually};
use serde_json::Value;
use uuid::Uuid;

const HEADER: &str = "date,merchant,amount,currency,category,note";

fn csv(rows: &[String]) -> String {
    format!("{HEADER}\n{}\n", rows.join("\n"))
}

/// Java: `goodRow(n)`.
fn good_row(n: usize) -> String {
    format!("2026-01-{:02},Merchant {n},12.50,EUR,MEALS,", 1 + n % 28)
}

fn imports(team: Uuid) -> String {
    format!("{V1}/teams/{team}/imports")
}

async fn upload_bytes(client: &Client, team: Uuid, bytes: impl Into<Vec<u8>>) -> Response {
    client
        .post(&imports(team))
        .file("file", "expenses.csv", "text/csv", bytes)
        .send()
        .await
}

async fn upload(client: &Client, team: &SignedInTeam, csv: &str) -> Response {
    upload_bytes(client, team.id, csv.as_bytes().to_vec()).await
}

fn id(response: &Response) -> String {
    response.json()["id"]
        .as_str()
        .unwrap_or_else(|| panic!("no id in {}", response.text()))
        .to_owned()
}

async fn get(client: &Client, team: Uuid, import: &str) -> Response {
    client.get(&format!("{}/{import}", imports(team))).send().await
}

/// Java: `awaitFinished()`: polls for up to 90 seconds until the import is COMPLETED or FAILED.
async fn await_finished(client: &Client, team: &SignedInTeam, import: &str) -> Value {
    eventually(
        &format!("import {import} to finish"),
        Duration::from_secs(90),
        || async {
            let response = get(client, team.id, import).await;
            let body = response.json();
            (response.status == 200 && (body["status"] == "COMPLETED" || body["status"] == "FAILED")).then_some(body)
        },
    )
    .await
}

async fn count(world: &World, sql: &'static str, team: Uuid) -> i64 {
    sqlx::query_scalar(sql)
        .bind(team)
        .fetch_one(world.fixtures.pool())
        .await
        .unwrap()
}

async fn expense_count(world: &World, team: &SignedInTeam) -> i64 {
    count(world, "select count(*) from expenses where team_id = $1", team.id).await
}

async fn job_count(world: &World, team: &SignedInTeam) -> i64 {
    count(world, "select count(*) from import_jobs where team_id = $1", team.id).await
}

mod m8 {
    use super::*;

    /// Java: `ImportApiTest.aTenThousandRowImportWithThirtySevenBadRowsReportsExactlyThoseLines`.
    #[tokio::test]
    #[ignore = "M8"]
    async fn a_ten_thousand_row_import_with_thirty_seven_bad_rows_reports_exactly_those_lines() {
        let world = World::start().await;
        let team = world.team().await;
        let importer = world.as_user(&team.member);
        let long_merchant = format!("2026-01-05,{},5.00,EUR,MEALS,", "m".repeat(201));
        let bad = [
            "2026-01-05,Cafe,abc,EUR,MEALS,",
            "2026-01-05,Cafe,1.234,EUR,MEALS,",
            "2026-13-45,Cafe,5.00,EUR,MEALS,",
            "2999-01-01,Cafe,5.00,EUR,MEALS,",
            "2026-01-05,Cafe,5.00,ZZZ,MEALS,",
            "2026-01-05,Cafe,5.00,EUR,FOOD,",
            "2026-01-05,,5.00,EUR,MEALS,",
            "2026-01-05,only,three",
            "2026-01-05,Cafe,5.00,EUR,MEALS,note,extra",
            long_merchant.as_str(),
            "2026-01-05,Cafe,-5.00,EUR,MEALS,",
            "2026-01-05,Cafe,0,EUR,MEALS,",
        ];
        let mut expected_lines = BTreeSet::new();
        let mut rows = Vec::with_capacity(10_000);
        let mut bad_count = 0;
        for i in 1..=10_000 {
            if (i - 1) % 270 == 0 && bad_count < 37 {
                rows.push(bad[bad_count % bad.len()].to_owned());
                expected_lines.insert(i + 1); // the header is line 1
                bad_count += 1;
            } else {
                rows.push(good_row(i));
            }
        }
        assert_eq!(bad_count, 37);

        let queued = upload(&importer, &team, &csv(&rows)).await;

        assert_eq!(queued.status, 202);
        let import = id(&queued);
        assert!(
            queued
                .header("location")
                .unwrap()
                .ends_with(&format!("{}/{import}", imports(team.id))),
            "{:?}",
            queued.header("location")
        );
        assert_eq!(queued.json()["totalRows"], 10_000);
        let done = await_finished(&importer, &team, &import).await;
        assert_eq!(done["status"], "COMPLETED");
        assert_eq!(done["totalRows"], 10_000);
        assert_eq!(done["importedRows"], 9_963);
        assert_eq!(done["skippedRows"], 37);
        assert_eq!(done["errorsTruncated"], false);
        let reported: BTreeSet<usize> = done["errors"]
            .as_array()
            .unwrap()
            .iter()
            .map(|error| usize::try_from(error["line"].as_u64().unwrap()).unwrap())
            .collect();
        assert_eq!(reported, expected_lines);
        assert_eq!(expense_count(&world, &team).await, 9_963);
        let drafts: i64 = sqlx::query_scalar(
            "select count(*) from expenses where team_id = $1 and status = 'DRAFT' and claimant_id = $2",
        )
        .bind(team.id)
        .bind(team.member.id())
        .fetch_one(world.fixtures.pool())
        .await
        .unwrap();
        assert_eq!(drafts, 9_963);
    }

    /// Java: `ImportApiTest.errorsNameTheKindOfProblemAndTheLine`.
    #[tokio::test]
    #[ignore = "M8"]
    async fn errors_name_the_kind_of_problem_and_the_line() {
        let world = World::start().await;
        let team = world.team().await;
        let importer = world.as_user(&team.member);
        let rows = [
            good_row(1),
            "2026-01-05,Cafe,abc,EUR,MEALS,".to_owned(),
            "2026-13-45,Cafe,5.00,EUR,MEALS,".to_owned(),
            good_row(4),
        ];

        let queued = upload(&importer, &team, &csv(&rows)).await;
        let done = await_finished(&importer, &team, &id(&queued)).await;

        assert_eq!(done["importedRows"], 2);
        assert_eq!(done["errors"][0]["line"], 3);
        assert_eq!(done["errors"][0]["code"], "AMOUNT_INVALID");
        assert_eq!(done["errors"][1]["line"], 4);
        assert_eq!(done["errors"][1]["code"], "DATE_INVALID");
    }

    /// Java: `ImportApiTest.quotesBlankLinesCrlfAndABomAreHandled`.
    #[tokio::test]
    #[ignore = "M8"]
    async fn quotes_blank_lines_crlf_and_a_bom_are_handled() {
        let world = World::start().await;
        let team = world.team().await;
        let importer = world.as_user(&team.member);
        let content = format!(
            "\u{feff}{HEADER}\r\n\
             2026-01-02,\"Smith, Jones & Co\",20.00,EUR,OFFICE,\"a note, with comma\"\r\n\
             \r\n\
             2026-01-03,=cmd|' /C calc'!A0,5.00,eur,travel,\r\n"
        );

        let queued = upload(&importer, &team, &content).await;

        assert_eq!(queued.status, 202);
        assert_eq!(queued.json()["totalRows"], 2, "the blank line is not a row");
        let done = await_finished(&importer, &team, &id(&queued)).await;
        assert_eq!(done["importedRows"], 2);
        let merchants: Vec<String> =
            sqlx::query_scalar("select merchant from expenses where team_id = $1 order by merchant")
                .bind(team.id)
                .fetch_all(world.fixtures.pool())
                .await
                .unwrap();
        assert_eq!(merchants, ["=cmd|' /C calc'!A0", "Smith, Jones & Co"]);
        let note: String =
            sqlx::query_scalar("select note from expenses where team_id = $1 and merchant like 'Smith%'")
                .bind(team.id)
                .fetch_one(world.fixtures.pool())
                .await
                .unwrap();
        assert_eq!(note, "a note, with comma");
    }

    /// Java: `ImportApiTest.aLineWithAnUnbalancedQuoteIsReportedAndTheRestStillImports`.
    #[tokio::test]
    #[ignore = "M8"]
    async fn a_line_with_an_unbalanced_quote_is_reported_and_the_rest_still_imports() {
        let world = World::start().await;
        let team = world.team().await;
        let importer = world.as_user(&team.member);
        let rows = [
            good_row(1),
            "2026-01-05,\"unterminated,5.00,EUR,MEALS,".to_owned(),
            good_row(3),
        ];

        let queued = upload(&importer, &team, &csv(&rows)).await;
        let done = await_finished(&importer, &team, &id(&queued)).await;

        assert_eq!(done["status"], "COMPLETED");
        assert_eq!(done["importedRows"], 2);
        assert_eq!(done["errors"][0]["line"], 3);
    }

    /// Java: `ImportApiTest.moreThanTenThousandRowsGets400AndNothingIsQueued`.
    #[tokio::test]
    #[ignore = "M8"]
    async fn more_than_ten_thousand_rows_gets_400_and_nothing_is_queued() {
        let world = World::start().await;
        let team = world.team().await;
        let rows: Vec<String> = (1..=10_001).map(good_row).collect();

        let response = upload(&world.as_user(&team.member), &team, &csv(&rows)).await;

        assert_eq!(response.status, 400);
        assert_eq!(job_count(&world, &team).await, 0);
        assert_eq!(expense_count(&world, &team).await, 0);
    }

    /// Java: `ImportApiTest.exactlyTenThousandRowsIsAccepted`.
    #[tokio::test]
    #[ignore = "M8"]
    async fn exactly_ten_thousand_rows_is_accepted() {
        let world = World::start().await;
        let team = world.team().await;
        let rows: Vec<String> = (1..=10_000).map(good_row).collect();

        assert_eq!(
            upload(&world.as_user(&team.member), &team, &csv(&rows)).await.status,
            202
        );
    }

    /// Java: `ImportApiTest.aSixthImportOfTheDayGets429WithRetryAfter_API6`.
    #[tokio::test]
    #[ignore = "M8"]
    async fn a_sixth_import_of_the_day_gets_429_with_retry_after() {
        let world = World::start().await;
        let team = world.team().await;
        let importer = world.as_user(&team.member);
        for i in 0..5 {
            assert_eq!(upload(&importer, &team, &csv(&[good_row(i)])).await.status, 202);
        }

        let sixth = upload(&importer, &team, &csv(&[good_row(9)])).await;

        assert_eq!(sixth.status, 429);
        assert_eq!(sixth.problem_code().as_deref(), Some("RATE_LIMITED"));
        let retry_after: u64 = sixth.header("retry-after").unwrap().parse().unwrap();
        assert!((1..=86_400).contains(&retry_after), "{retry_after}");
        assert_eq!(job_count(&world, &team).await, 5);
    }

    /// Java: `ImportApiTest.theLimitIsPerUserAndPerTeamAndResetsTomorrow`, without the last step (the next day),
    /// which needs a clock.
    #[tokio::test]
    #[ignore = "M8"]
    async fn the_limit_is_per_user_and_per_team() {
        let world = World::start().await;
        let team = world.team().await;
        let importer = world.as_user(&team.member);
        for i in 0..5 {
            upload(&importer, &team, &csv(&[good_row(i)])).await;
        }
        assert_eq!(upload(&importer, &team, &csv(&[good_row(9)])).await.status, 429);

        let other = upload(&world.as_user(&team.colleague), &team, &csv(&[good_row(1)])).await;
        let second_team = world.fixtures.team().await;
        world.fixtures.member(second_team, &team.member.user, "MEMBER").await;
        let elsewhere = upload_bytes(&importer, second_team, csv(&[good_row(1)])).await;

        assert_eq!(other.status, 202);
        assert_eq!(elsewhere.status, 202);
    }

    /// Java: `ImportApiTest.eightSimultaneousUploadsLetExactlyFiveThrough`.
    #[tokio::test]
    #[ignore = "M8"]
    async fn eight_simultaneous_uploads_let_exactly_five_through() {
        let world = World::start().await;
        let team = world.team().await;
        let importer = world.as_user(&team.member);
        let uploads: Vec<_> = (0..8)
            .map(|n| {
                let (importer, body) = (importer.clone(), csv(&[good_row(n)]));
                tokio::spawn(async move { upload_bytes(&importer, team.id, body).await.status })
            })
            .collect();

        let mut statuses = Vec::new();
        for upload in uploads {
            statuses.push(upload.await.unwrap());
        }

        assert_eq!(
            statuses.iter().filter(|status| **status == 202).count(),
            5,
            "{statuses:?}"
        );
        assert_eq!(
            statuses.iter().filter(|status| **status == 429).count(),
            3,
            "{statuses:?}"
        );
    }

    /// Java: `ImportApiTest.rejectedUploadsDoNotUseUpTheDailyAllowance`.
    #[tokio::test]
    #[ignore = "M8"]
    async fn rejected_uploads_do_not_use_up_the_daily_allowance() {
        let world = World::start().await;
        let team = world.team().await;
        let importer = world.as_user(&team.member);
        for _ in 0..6 {
            assert_eq!(
                upload(&importer, &team, "not,the,right,header\n1,2,3,4\n").await.status,
                400
            );
        }

        assert_eq!(upload(&importer, &team, &csv(&[good_row(1)])).await.status, 202);
    }

    /// Java: `ImportApiTest.wrongHeaderEmptyAndHeaderOnlyFilesGet400`.
    #[tokio::test]
    #[ignore = "M8"]
    async fn wrong_header_empty_and_header_only_files_get_400() {
        let world = World::start().await;
        let team = world.team().await;
        let importer = world.as_user(&team.member);

        for content in [
            "a,b,c\n1,2,3\n".to_owned(),
            String::new(),
            format!("{HEADER}\n"),
            "date,merchant,amount,currency\n2026-01-01,x,1,EUR\n".to_owned(),
        ] {
            assert_eq!(upload(&importer, &team, &content).await.status, 400, "{content:?}");
        }
    }

    /// Java: `ImportApiTest.theNoteColumnIsOptional`.
    #[tokio::test]
    #[ignore = "M8"]
    async fn the_note_column_is_optional() {
        let world = World::start().await;
        let team = world.team().await;
        let importer = world.as_user(&team.member);

        let queued = upload(
            &importer,
            &team,
            "date,merchant,amount,currency,category\n2026-01-02,Cafe,9.90,EUR,MEALS\n",
        )
        .await;

        assert_eq!(queued.status, 202);
        assert_eq!(await_finished(&importer, &team, &id(&queued)).await["importedRows"], 1);
    }

    /// Java: `ImportApiTest.binaryFilesGet415`.
    #[tokio::test]
    #[ignore = "M8"]
    async fn binary_files_get_415() {
        let world = World::start().await;
        let team = world.team().await;
        let importer = world.as_user(&team.member);
        // ISO-8859-1: "é" is the single byte 0xE9, which is not UTF-8.
        let mut not_utf8 = format!("{HEADER}\n2026-01-01,Caf").into_bytes();
        not_utf8.push(0xE9);
        not_utf8.extend_from_slice(b",1.00,EUR,MEALS,\n");

        assert_eq!(upload_bytes(&importer, team.id, EXE).await.status, 415);
        assert_eq!(upload_bytes(&importer, team.id, not_utf8).await.status, 415);
    }

    /// Java: `ImportApiTest.aFileOverTenMegabytesGets413`.
    #[tokio::test]
    #[ignore = "M8"]
    async fn a_file_over_ten_megabytes_gets_413() {
        let world = World::start().await;
        let team = world.team().await;

        let big = vec![b'a'; 10 * 1024 * 1024 + 1];

        assert_eq!(
            upload_bytes(&world.as_user(&team.member), team.id, big).await.status,
            413
        );
    }

    /// Java: `ImportApiTest.onlyTeammatesCanImportAndAnonymousCannot`.
    #[tokio::test]
    #[ignore = "M8"]
    async fn only_teammates_can_import_and_anonymous_cannot() {
        let world = World::start().await;
        let team = world.team().await;
        let outsider = world.as_user(&world.user().await);

        assert_eq!(upload(&outsider, &team, &csv(&[good_row(1)])).await.status, 404);
        assert_eq!(
            upload(&world.anonymous(), &team, &csv(&[good_row(1)])).await.status,
            401
        );
    }

    /// Java: `ImportApiTest.anImportIsVisibleToItsStarterAndToOwnersOnly`.
    #[tokio::test]
    #[ignore = "M8"]
    async fn an_import_is_visible_to_its_starter_and_to_owners_only() {
        let world = World::start().await;
        let team = world.team().await;
        let importer = world.as_user(&team.member);
        let import = id(&upload(&importer, &team, &csv(&[good_row(1)])).await);
        await_finished(&importer, &team, &import).await;
        let outsider = world.as_user(&world.user().await);

        for (client, expected) in [
            (importer.clone(), 200),
            (world.as_user(&team.owner), 200),
            (world.as_user(&team.colleague), 404),
            (world.as_user(&team.approver), 404),
            (outsider, 404),
            (world.anonymous(), 401),
        ] {
            assert_eq!(get(&client, team.id, &import).await.status, expected);
        }
        assert_eq!(get(&importer, team.id, &Uuid::new_v4().to_string()).await.status, 404);
    }

    /// Java: `ImportApiTest.anImportIsNotReachableThroughAnotherTeam`.
    #[tokio::test]
    #[ignore = "M8"]
    async fn an_import_is_not_reachable_through_another_team() {
        let world = World::start().await;
        let team = world.team().await;
        let import = id(&upload(&world.as_user(&team.member), &team, &csv(&[good_row(1)])).await);
        let spy = world.user().await;
        let other_team = world.fixtures.team().await;
        world.fixtures.member(other_team, &spy.user, "OWNER").await;

        assert_eq!(get(&world.as_user(&spy), other_team, &import).await.status, 404);
    }
}
