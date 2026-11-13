//! Receipt files: upload, type detection, size limits, malware scan, names, duplicates, who may do what, and
//! what happens when they or their expense are deleted.
//!
//! Java: `src/test/java/com/ledgerly/receipts/internal/web/ReceiptsApiTest.java`. Java uses a file-system
//! object store and a fake scanner; here they are SeaweedFS and ClamAV from `compose.yaml`, which recognises
//! the EICAR file. Its `claimant` and `colleague` are the team's MEMBER and colleague here.
//!
//! Elsewhere: the tests with the scanner switched off are in `receipts_scanner_outage.rs`. The presigned link
//! is checked through its URL, since the store's record of presigned links is internal to Java's test.

// clippy.toml exempts `#[test]` functions only; the helpers of a test file are test code too.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

use std::time::Duration;

use chrono::{DateTime, Utc};
use ledgerly_parity_tests::encoding::query_parameter;
use ledgerly_parity_tests::fixtures::{EXE, infected_pdf};
use ledgerly_parity_tests::http::{Client, Response};
use ledgerly_parity_tests::{Session, SignedInTeam, World, eventually};
use uuid::Uuid;

/// Java: `ReceiptsApiTest.PDF`, `PNG` and `JPEG`: the first bytes of each type.
const PDF: &[u8] = b"%PDF-1.4\n1 0 obj << /Type /Catalog >> endobj\n%%EOF\n";
const PNG: &[u8] = &[
    0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 0x0D, b'I', b'H', b'D', b'R',
];
const JPEG: &[u8] = &[0xFF, 0xD8, 0xFF, 0xE0, 0, 0x10, b'J', b'F', b'I', b'F', 0];

fn base(team: &SignedInTeam, expense: Uuid) -> String {
    format!("{}/receipts", team.expense_path(expense))
}

async fn upload(client: &Client, path: &str, name: &str, bytes: &[u8], declared: &str) -> Response {
    client
        .post(path)
        .file("file", name, declared, bytes.to_vec())
        .send()
        .await
}

fn id(response: &Response) -> String {
    response.json()["id"].as_str().unwrap().to_owned()
}

/// Java: `uploaded()`: the PDF, uploaded by the claimant.
async fn uploaded(world: &World, team: &SignedInTeam, expense: Uuid) -> String {
    let response = upload(
        &world.as_user(&team.member),
        &base(team, expense),
        "lunch.pdf",
        PDF,
        "application/pdf",
    )
    .await;
    assert_eq!(response.status, 202);
    id(&response)
}

/// Java: `statusOf()`: the receipt's status in the claimant's list, or `GONE`.
async fn status_of(world: &World, team: &SignedInTeam, expense: Uuid, receipt: &str) -> String {
    let list = world
        .as_user(&team.member)
        .get(&base(team, expense))
        .send()
        .await
        .json();
    list.as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["id"] == receipt)
        .map_or_else(
            || "GONE".to_owned(),
            |entry| entry["status"].as_str().unwrap().to_owned(),
        )
}

/// Java: `awaitStatus()`: polls for up to 15 seconds.
async fn await_status(world: &World, team: &SignedInTeam, expense: Uuid, receipt: &str, expected: &str) {
    eventually(
        &format!("receipt {receipt} to be {expected}"),
        Duration::from_secs(15),
        || async { (status_of(world, team, expense, receipt).await == expected).then_some(()) },
    )
    .await;
}

async fn list_len(client: &Client, path: &str) -> usize {
    client.get(path).send().await.json().as_array().unwrap().len()
}

/// An expense of `claimant` in another team, with `claimant` as its MEMBER.
async fn expense_in_another_team(world: &World, claimant: &Session, role: &str) -> (Uuid, Uuid) {
    let team = world.fixtures.team().await;
    world.fixtures.member(team, &claimant.user, role).await;
    let expense = world.fixtures.expense(team, &claimant.user, "DRAFT").await;
    (team, expense)
}

mod m7 {
    use super::*;

    // --- happy path ---

    /// Java: `ReceiptsApiTest.uploadIsAcceptedThenScannedAndBecomesDownloadable`.
    #[tokio::test]
    #[ignore = "M7"]
    async fn upload_is_accepted_then_scanned_and_becomes_downloadable() {
        let world = World::start().await;
        let team = world.team().await;
        let e = world.expense(&team, &team.member, "DRAFT").await;
        let claimant = world.as_user(&team.member);

        let response = upload(&claimant, &base(&team, e), "lunch receipt.pdf", PDF, "application/pdf").await;

        assert_eq!(response.status, 202);
        let body = response.json();
        assert_eq!(body["status"], "PENDING_SCAN");
        assert_eq!(body["detectedType"], "application/pdf");
        assert_eq!(body["sizeBytes"], PDF.len());
        assert_eq!(body["possibleDuplicate"], false);
        // The Location is the list of the expense's receipts, not the receipt.
        assert!(response.header("location").unwrap().ends_with(&base(&team, e)));
        await_status(&world, &team, e, &id(&response), "ACCEPTED").await;

        let link = claimant
            .get(&format!("{}/{}/download", base(&team, e), id(&response)))
            .send()
            .await;
        assert_eq!(link.status, 200);
        assert!(link.json()["url"].as_str().unwrap().contains("/receipts/"));
        let expires_at: DateTime<Utc> = link.json()["expiresAt"].as_str().unwrap().parse().unwrap();
        assert!(expires_at < Utc::now() + chrono::Duration::minutes(5) + chrono::Duration::seconds(5));
        assert_eq!(link.header("cache-control"), Some("no-store"));
    }

    /// Java: `ReceiptsApiTest.jpegAndPngAreAcceptedToo`.
    #[tokio::test]
    #[ignore = "M7"]
    async fn jpeg_and_png_are_accepted_too() {
        let world = World::start().await;
        let team = world.team().await;
        let e = world.expense(&team, &team.member, "DRAFT").await;
        let claimant = world.as_user(&team.member);

        let jpeg = upload(&claimant, &base(&team, e), "a.jpg", JPEG, "image/jpeg").await;
        let png = upload(&claimant, &base(&team, e), "b.png", PNG, "image/png").await;

        assert_eq!(jpeg.json()["detectedType"], "image/jpeg");
        assert_eq!(png.json()["detectedType"], "image/png");
    }

    /// Java: `ReceiptsApiTest.theTypeComesFromTheBytesNotFromTheDeclaredContentType`.
    #[tokio::test]
    #[ignore = "M7"]
    async fn the_type_comes_from_the_bytes_not_from_the_declared_content_type() {
        let world = World::start().await;
        let team = world.team().await;
        let e = world.expense(&team, &team.member, "DRAFT").await;

        let response = upload(
            &world.as_user(&team.member),
            &base(&team, e),
            "photo.png",
            PDF,
            "image/png",
        )
        .await;

        assert_eq!(response.status, 202);
        assert_eq!(response.json()["detectedType"], "application/pdf");
    }

    // --- abuse: file content (API4, API8) ---

    /// Java: `ReceiptsApiTest.anExeRenamedToPdfGets415`. Java also counts the objects in its store; here the
    /// list stays empty and no receipt row exists.
    #[tokio::test]
    #[ignore = "M7"]
    async fn an_exe_renamed_to_pdf_gets_415() {
        let world = World::start().await;
        let team = world.team().await;
        let e = world.expense(&team, &team.member, "DRAFT").await;
        let claimant = world.as_user(&team.member);

        let response = upload(&claimant, &base(&team, e), "invoice.pdf", EXE, "application/pdf").await;

        assert_eq!(response.status, 415);
        assert_eq!(response.problem_code().as_deref(), Some("UNSUPPORTED_MEDIA_TYPE"));
        assert_eq!(list_len(&claimant, &base(&team, e)).await, 0);
        let rows: i64 = sqlx::query_scalar("select count(*) from receipts where expense_id = $1")
            .bind(e)
            .fetch_one(world.fixtures.pool())
            .await
            .unwrap();
        assert_eq!(rows, 0);
    }

    /// Java: `ReceiptsApiTest.plainTextAndHtmlAreRejectedWhateverTheyClaim`.
    #[tokio::test]
    #[ignore = "M7"]
    async fn plain_text_and_html_are_rejected_whatever_they_claim() {
        let world = World::start().await;
        let team = world.team().await;
        let e = world.expense(&team, &team.member, "DRAFT").await;
        let claimant = world.as_user(&team.member);

        let text = upload(&claimant, &base(&team, e), "a.pdf", b"hello", "application/pdf").await;
        let html = upload(
            &claimant,
            &base(&team, e),
            "a.png",
            b"<html><script>alert(1)</script></html>",
            "image/png",
        )
        .await;

        assert_eq!(text.status, 415);
        assert_eq!(html.status, 415);
    }

    /// Java: `ReceiptsApiTest.anEmptyFileGets400`.
    #[tokio::test]
    #[ignore = "M7"]
    async fn an_empty_file_gets_400() {
        let world = World::start().await;
        let team = world.team().await;
        let e = world.expense(&team, &team.member, "DRAFT").await;

        let response = upload(
            &world.as_user(&team.member),
            &base(&team, e),
            "empty.pdf",
            b"",
            "application/pdf",
        )
        .await;

        assert_eq!(response.status, 400);
    }

    /// Java: `ReceiptsApiTest.tenMegabytesPlusOneByteGets413`.
    #[tokio::test]
    #[ignore = "M7"]
    async fn ten_megabytes_plus_one_byte_gets_413() {
        let world = World::start().await;
        let team = world.team().await;
        let e = world.expense(&team, &team.member, "DRAFT").await;
        let mut big = PDF.to_vec();
        big.resize(10 * 1024 * 1024 + 1, 0);

        let response = upload(
            &world.as_user(&team.member),
            &base(&team, e),
            "big.pdf",
            &big,
            "application/pdf",
        )
        .await;

        assert_eq!(response.status, 413);
        assert_eq!(response.problem_code().as_deref(), Some("PAYLOAD_TOO_LARGE"));
    }

    /// Java: `ReceiptsApiTest.exactlyTenMegabytesIsAccepted`.
    #[tokio::test]
    #[ignore = "M7"]
    async fn exactly_ten_megabytes_is_accepted() {
        let world = World::start().await;
        let team = world.team().await;
        let e = world.expense(&team, &team.member, "DRAFT").await;
        let mut max = PDF.to_vec();
        max.resize(10 * 1024 * 1024, 0);

        let response = upload(
            &world.as_user(&team.member),
            &base(&team, e),
            "max.pdf",
            &max,
            "application/pdf",
        )
        .await;

        assert_eq!(response.status, 202);
    }

    /// Java: `ReceiptsApiTest.aMissingFilePartGets400`.
    #[tokio::test]
    #[ignore = "M7"]
    async fn a_missing_file_part_gets_400() {
        let world = World::start().await;
        let team = world.team().await;
        let e = world.expense(&team, &team.member, "DRAFT").await;

        let response = world
            .as_user(&team.member)
            .post(&base(&team, e))
            .file("other", "x.txt", "text/plain", b"x".to_vec())
            .send()
            .await;

        assert_eq!(response.status, 400);
    }

    // --- malware ---

    /// Java: `ReceiptsApiTest.anEicarFileIsQuarantinedAndNeverServed`. Java also checks that the key was
    /// never presigned, which only its fake store records.
    #[tokio::test]
    #[ignore = "M7"]
    async fn an_eicar_file_is_quarantined_and_never_served() {
        let world = World::start().await;
        let team = world.team().await;
        let e = world.expense(&team, &team.member, "DRAFT").await;

        let response = upload(
            &world.as_user(&team.member),
            &base(&team, e),
            "invoice.pdf",
            &infected_pdf(),
            "application/pdf",
        )
        .await;
        assert_eq!(response.status, 202);
        let receipt = id(&response);
        await_status(&world, &team, e, &receipt, "QUARANTINED").await;

        let download = format!("{}/{receipt}/download", base(&team, e));
        assert_eq!(world.as_user(&team.member).get(&download).send().await.status, 404);
        assert_eq!(world.as_user(&team.approver).get(&download).send().await.status, 404);
        let key = world.fixtures.storage_key(receipt.parse().unwrap()).await;
        assert!(key.starts_with("quarantine/"), "{key}");
        assert!(world.fixtures.object_exists(&key).await);
    }

    /// Java: `ReceiptsApiTest.aQuarantinedReceiptDoesNotCountTowardsSubmitting`.
    #[tokio::test]
    #[ignore = "M7"]
    async fn a_quarantined_receipt_does_not_count_towards_submitting() {
        let world = World::start().await;
        let team = world.team().await;
        let e = world.expense(&team, &team.member, "DRAFT").await;
        let claimant = world.as_user(&team.member);
        let response = upload(
            &claimant,
            &base(&team, e),
            "invoice.pdf",
            &infected_pdf(),
            "application/pdf",
        )
        .await;
        await_status(&world, &team, e, &id(&response), "QUARANTINED").await;

        let submit = claimant.post(&format!("{}:submit", team.expense_path(e))).send().await;

        assert_eq!(submit.status, 422);
    }

    // --- names and storage keys (path traversal) ---

    /// Java: `ReceiptsApiTest.aTraversalFileNameIsOnlyMetadata`. Java reads the download name, lifetime and
    /// type of the presigned link from its fake store; here they are read from the link's query parameters.
    #[tokio::test]
    #[ignore = "M7"]
    async fn a_traversal_file_name_is_only_metadata() {
        let world = World::start().await;
        let team = world.team().await;
        let e = world.expense(&team, &team.member, "DRAFT").await;
        let claimant = world.as_user(&team.member);

        let response = upload(&claimant, &base(&team, e), "../../etc/passwd", PDF, "application/pdf").await;

        assert_eq!(response.status, 202);
        assert_eq!(response.json()["originalName"], "../../etc/passwd");
        let receipt = id(&response);
        let key = world.fixtures.storage_key(receipt.parse().unwrap()).await;
        let suffix = key.strip_prefix("receipts/").unwrap_or_else(|| panic!("{key}"));
        assert!(suffix.parse::<Uuid>().is_ok() && suffix.len() == 36, "{key}");
        await_status(&world, &team, e, &receipt, "ACCEPTED").await;
        let url = claimant
            .get(&format!("{}/{receipt}/download", base(&team, e)))
            .send()
            .await
            .json()["url"]
            .as_str()
            .unwrap()
            .to_owned();
        let disposition = query_parameter(&url, "response-content-disposition").unwrap_or_else(|| panic!("{url}"));
        assert!(disposition.contains("passwd.pdf"), "{disposition}");
        let ttl: u64 = query_parameter(&url, "X-Amz-Expires").unwrap().parse().unwrap();
        assert!(ttl <= 300, "{ttl}");
        assert_eq!(
            query_parameter(&url, "response-content-type").as_deref(),
            Some("application/pdf")
        );
    }

    /// Java: `ReceiptsApiTest.controlCharactersInTheFileNameAreDropped`.
    #[tokio::test]
    #[ignore = "M7"]
    async fn control_characters_in_the_file_name_are_dropped() {
        let world = World::start().await;
        let team = world.team().await;
        let e = world.expense(&team, &team.member, "DRAFT").await;

        let response = upload(
            &world.as_user(&team.member),
            &base(&team, e),
            "nice\tname.pdf",
            PDF,
            "application/pdf",
        )
        .await;

        assert_eq!(response.json()["originalName"], "nicename.pdf");
    }

    // --- duplicates ---

    /// Java: `ReceiptsApiTest.theSameFileTwiceOnOneExpenseGets409`.
    #[tokio::test]
    #[ignore = "M7"]
    async fn the_same_file_twice_on_one_expense_gets_409() {
        let world = World::start().await;
        let team = world.team().await;
        let e = world.expense(&team, &team.member, "DRAFT").await;
        uploaded(&world, &team, e).await;
        let claimant = world.as_user(&team.member);

        let again = upload(&claimant, &base(&team, e), "again.pdf", PDF, "application/pdf").await;

        assert_eq!(again.status, 409);
        assert_eq!(again.problem_code().as_deref(), Some("RECEIPT_DUPLICATE"));
        assert_eq!(list_len(&claimant, &base(&team, e)).await, 1);
    }

    /// Java: `ReceiptsApiTest.theSameFileOnASiblingExpenseIsFlaggedNotBlocked`.
    #[tokio::test]
    #[ignore = "M7"]
    async fn the_same_file_on_a_sibling_expense_is_flagged_not_blocked() {
        let world = World::start().await;
        let team = world.team().await;
        let first = world.expense(&team, &team.member, "DRAFT").await;
        let second = world.expense(&team, &team.member, "DRAFT").await;
        uploaded(&world, &team, first).await;

        let response = upload(
            &world.as_user(&team.member),
            &base(&team, second),
            "same.pdf",
            PDF,
            "application/pdf",
        )
        .await;

        assert_eq!(response.status, 202);
        assert_eq!(response.json()["possibleDuplicate"], true);
    }

    /// Java: `ReceiptsApiTest.identicalFilesInAnotherTeamAreNotFlagged`.
    #[tokio::test]
    #[ignore = "M7"]
    async fn identical_files_in_another_team_are_not_flagged() {
        let world = World::start().await;
        let team = world.team().await;
        let mine = world.expense(&team, &team.member, "DRAFT").await;
        uploaded(&world, &team, mine).await;
        // Another team, another claimant, the same bytes.
        let other_claimant = world.user().await;
        let (other_team, theirs) = expense_in_another_team(&world, &other_claimant, "MEMBER").await;

        let response = upload(
            &world.as_user(&other_claimant),
            &format!("/api/v1/teams/{other_team}/expenses/{theirs}/receipts"),
            "x.pdf",
            PDF,
            "application/pdf",
        )
        .await;

        assert_eq!(response.status, 202);
        assert_eq!(response.json()["possibleDuplicate"], false);
    }

    /// Java: `ReceiptsApiTest.twoSimultaneousIdenticalUploadsLeaveExactlyOne`.
    #[tokio::test]
    #[ignore = "M7"]
    async fn two_simultaneous_identical_uploads_leave_exactly_one() {
        let world = World::start().await;
        let team = world.team().await;
        let e = world.expense(&team, &team.member, "DRAFT").await;
        let claimant = world.as_user(&team.member);
        let path = base(&team, e);

        let (a, b) = tokio::join!(
            upload(&claimant, &path, "x.pdf", PDF, "application/pdf"),
            upload(&claimant, &path, "x.pdf", PDF, "application/pdf"),
        );

        let mut statuses = [a.status, b.status];
        statuses.sort_unstable();
        assert_eq!(statuses, [202, 409]);
        assert_eq!(list_len(&claimant, &path).await, 1);
    }

    // --- authorization (API1, API5) ---

    /// Java: `ReceiptsApiTest.uploadingToASubmittedExpenseGets409`.
    #[tokio::test]
    #[ignore = "M7"]
    async fn uploading_to_a_submitted_expense_gets_409() {
        let world = World::start().await;
        let team = world.team().await;
        let e = world.expense(&team, &team.member, "SUBMITTED").await;

        let response = upload(
            &world.as_user(&team.member),
            &base(&team, e),
            "late.pdf",
            PDF,
            "application/pdf",
        )
        .await;

        assert_eq!(response.status, 409);
        assert_eq!(response.problem_code().as_deref(), Some("EXPENSE_NOT_DRAFT"));
    }

    /// Java: `ReceiptsApiTest.onlyTheClaimantMayUpload`.
    #[tokio::test]
    #[ignore = "M7"]
    async fn only_the_claimant_may_upload() {
        let world = World::start().await;
        let team = world.team().await;
        let e = world.expense(&team, &team.member, "DRAFT").await;
        let path = base(&team, e);
        let outsider = world.user().await;

        for (client, expected) in [
            (world.as_user(&team.approver), 403),
            (world.as_user(&team.colleague), 404),
            (world.as_user(&outsider), 404),
            (world.anonymous(), 401),
        ] {
            assert_eq!(
                upload(&client, &path, "a.pdf", PDF, "application/pdf").await.status,
                expected
            );
        }
    }

    /// Java: `ReceiptsApiTest.aTeamBReceiptIdGets404FromTeamA_API1`.
    #[tokio::test]
    #[ignore = "M7"]
    async fn a_receipt_id_of_another_team_gets_404() {
        let world = World::start().await;
        let team = world.team().await;
        let e = world.expense(&team, &team.member, "DRAFT").await;
        let receipt = uploaded(&world, &team, e).await;
        await_status(&world, &team, e, &receipt, "ACCEPTED").await;
        let spy = world.user().await;
        let other_team = world.fixtures.team().await;
        world.fixtures.member(other_team, &spy.user, "OWNER").await;
        let spy_client = world.as_user(&spy);

        // Through the other team's path, with the right expense and receipt ids.
        let receipts = format!("/api/v1/teams/{other_team}/expenses/{e}/receipts");
        assert_eq!(
            spy_client
                .get(&format!("{receipts}/{receipt}/download"))
                .send()
                .await
                .status,
            404
        );
        assert_eq!(spy_client.get(&receipts).send().await.status, 404);
    }

    /// Java: `ReceiptsApiTest.aReceiptIdFromAnotherExpenseOfTheSameTeamGets404`.
    #[tokio::test]
    #[ignore = "M7"]
    async fn a_receipt_id_from_another_expense_of_the_same_team_gets_404() {
        let world = World::start().await;
        let team = world.team().await;
        let first = world.expense(&team, &team.member, "DRAFT").await;
        let second = world.expense(&team, &team.member, "DRAFT").await;
        let receipt = uploaded(&world, &team, first).await;
        await_status(&world, &team, first, &receipt, "ACCEPTED").await;
        let claimant = world.as_user(&team.member);

        let wrong = format!("{}/{receipt}", base(&team, second));
        assert_eq!(claimant.get(&format!("{wrong}/download")).send().await.status, 404);
        assert_eq!(claimant.delete(&wrong).send().await.status, 404);
    }

    /// Java: `ReceiptsApiTest.receiptsAreVisibleLikeTheirExpense`.
    #[tokio::test]
    #[ignore = "M7"]
    async fn receipts_are_visible_like_their_expense() {
        let world = World::start().await;
        let team = world.team().await;
        let e = world.expense(&team, &team.member, "DRAFT").await;
        let receipt = uploaded(&world, &team, e).await;
        await_status(&world, &team, e, &receipt, "ACCEPTED").await;
        let list = base(&team, e);
        let download = format!("{list}/{receipt}/download");
        let outsider = world.user().await;

        assert_eq!(world.as_user(&team.approver).get(&list).send().await.status, 200);
        assert_eq!(world.as_user(&team.approver).get(&download).send().await.status, 200);
        assert_eq!(world.as_user(&team.colleague).get(&list).send().await.status, 404);
        assert_eq!(world.as_user(&team.colleague).get(&download).send().await.status, 404);
        assert_eq!(world.as_user(&outsider).get(&list).send().await.status, 404);
        assert_eq!(world.anonymous().get(&list).send().await.status, 401);
    }

    // --- deleting ---

    /// Java: `ReceiptsApiTest.theClaimantCanDeleteAReceiptOfADraftAndItsObjectIsRemoved`.
    #[tokio::test]
    #[ignore = "M7"]
    async fn the_claimant_can_delete_a_receipt_of_a_draft_and_its_object_is_removed() {
        let world = World::start().await;
        let team = world.team().await;
        let e = world.expense(&team, &team.member, "DRAFT").await;
        let receipt = uploaded(&world, &team, e).await;
        let key = world.fixtures.storage_key(receipt.parse().unwrap()).await;
        let claimant = world.as_user(&team.member);
        let path = format!("{}/{receipt}", base(&team, e));

        assert_eq!(claimant.delete(&path).send().await.status, 204);
        assert_eq!(list_len(&claimant, &base(&team, e)).await, 0);
        assert!(!world.fixtures.object_exists(&key).await);
        assert_eq!(claimant.delete(&path).send().await.status, 404);
    }

    /// Java: `ReceiptsApiTest.receiptsCannotBeDeletedOnceTheExpenseIsSubmitted`.
    #[tokio::test]
    #[ignore = "M7"]
    async fn receipts_cannot_be_deleted_once_the_expense_is_submitted() {
        let world = World::start().await;
        let team = world.team().await;
        let e = world.expense(&team, &team.member, "DRAFT").await;
        let receipt = uploaded(&world, &team, e).await;
        await_status(&world, &team, e, &receipt, "ACCEPTED").await;
        sqlx::query("update expenses set status = 'SUBMITTED' where id = $1")
            .bind(e)
            .execute(world.fixtures.pool())
            .await
            .unwrap();
        let path = format!("{}/{receipt}", base(&team, e));

        assert_eq!(world.as_user(&team.member).delete(&path).send().await.status, 409);
        assert_eq!(world.as_user(&team.approver).delete(&path).send().await.status, 403);
    }

    /// Java: `ReceiptsApiTest.deletingTheExpenseRemovesItsReceiptsAndFiles`.
    #[tokio::test]
    #[ignore = "M7"]
    async fn deleting_the_expense_removes_its_receipts_and_files() {
        let world = World::start().await;
        let team = world.team().await;
        let e = world.expense(&team, &team.member, "DRAFT").await;
        let receipt = uploaded(&world, &team, e).await;
        let key = world.fixtures.storage_key(receipt.parse().unwrap()).await;
        let claimant = world.as_user(&team.member);
        let expense = team.expense_path(e);

        // DELETE needs If-Match ...
        assert_eq!(claimant.delete(&expense).send().await.status, 428);
        // ... so go through the API properly with the current ETag.
        let etag = claimant.get(&expense).send().await.header("etag").unwrap().to_owned();
        assert_eq!(
            claimant.delete(&expense).header("If-Match", &etag).send().await.status,
            204
        );

        eventually(
            "the receipts and their files to be removed",
            Duration::from_secs(10),
            || async {
                let rows: i64 = sqlx::query_scalar("select count(*) from receipts where expense_id = $1")
                    .bind(e)
                    .fetch_one(world.fixtures.pool())
                    .await
                    .unwrap();
                (rows == 0 && !world.fixtures.object_exists(&key).await).then_some(())
            },
        )
        .await;
    }
}
