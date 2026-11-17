# Java tests that are not parity tests

The parity suite talks only HTTP and SQL (ADR 0011). A Java test, or the part of one, that moves a clock,
injects a fault, or looks inside the process cannot be written that way. Each is listed here with the
milestone and the module whose in-process tests port it. The tests in `tests/architecture.rs`-style files of
those crates cite the Java test by name, so a reviewer can follow every row.

`tests/inventory.rs` checks that every test method in §1 and every test class in §2 of
`docs/reference/java-test-inventory.md` is named either by a parity test or in this file.

Kinds, as in the inventory: `clock` (time travel), `fault` (an injected failure), `INT` (internals: beans,
repositories, log capture, the registry).

## Whole Java tests

| Java test | Kind | Milestone | Ported in-process in |
| --- | --- | --- | --- |
| `Api7ServerSideRequestForgeryTest.onlyInfrastructureMayCallOut` | INT (ArchUnit) | M10 | `crates/app/tests/architecture.rs::only_infrastructure_may_call_out` |
| `Api7ServerSideRequestForgeryTest.theRuleReallyFailsForAClassThatBreaksIt` | INT | M10 | `crates/app/tests/architecture.rs::the_rule_really_fails_for_code_that_breaks_it` |
| `Api7ServerSideRequestForgeryTest.theSameCodeInAnInfrastructurePackageIsAllowed` | INT | M10 | `crates/app/tests/architecture.rs::the_same_code_in_an_adapter_or_a_test_is_allowed` |
| `Api8SecurityMisconfigurationTest.theAllowlistRefusesWildcardsPathsAndSloppyEntries` | INT | M1 | `crates/shared`: `config::tests::{exact_origins_are_valid, wildcards_paths_and_other_schemes_are_not_origins}`; `crates/app`: `config::tests::every_unusable_value_is_reported_at_once_by_variable` — done |
| `Api8SecurityMisconfigurationTest.anUnexpectedFailureRevealsNothingButATraceId` | fault (a test-only route) | M3 | `crates/app`: `tests::a_panic_is_a_500_whose_trace_id_is_the_one_in_the_log_line` — done |
| `Api9ImproperInventoryManagementTest.everyServedEndpointIsInTheSpecAndEveryDocumentedOneIsServed`, direction "served, therefore documented" | INT (route table) | M3 | `crates/app/tests/api_inventory.rs::every_served_endpoint_is_documented_and_every_documented_one_is_served_or_scheduled`; the other direction is `owasp_api9_inventory.rs` — done |
| `Api9ImproperInventoryManagementTest.theComparisonDetectsDriftInBothDirections` | INT | M3 | `crates/app/tests/api_inventory.rs::the_comparison_detects_drift_in_both_directions` — done |
| `Api9ImproperInventoryManagementTest.everyPathIsVersionedExceptTheKeyDiscoveryDocument` | INT | M5 | `crates/app/tests/api_inventory.rs::every_served_endpoint_is_documented_and_every_documented_one_is_served_or_scheduled` (every served path is a spec path, all under `/api/v1` but the JWKS) — done |
| `LedgerlyApplicationTest` | INT | M1 | `crates/shared`: `config::tests::limits_default_to_javas_values`; `crates/app`: `logging::tests::a_log_line_has_exactly_spring_boots_ecs_members` — done |
| `ModularityTest` | INT | M1 | `crates/app/tests/architecture.rs::{the_crate_graph_equals_the_allowed_arrows, the_allowed_arrows_themselves_pass}` — done |
| `ModuleDocumentationTest` | INT | M9 | nothing: it writes Modulith diagrams; CONCEPT §3 holds the diagram |
| `ProductionEnvironmentCheckTest` | INT | M1 | `crates/app`: `config::tests::{prod_with_nothing_set_reports_every_missing_setting_at_once, blank_values_count_as_missing, prod_with_everything_set_starts, with_keycloak_no_signing_keys_are_needed, the_failure_reads_as_an_actionable_message}` and one `prod_needs_…`/`keycloak_needs_…` test per variable (M11.5); `crates/app/tests/startup.rs::prod_without_configuration_exits_non_zero_and_lists_every_missing_variable` — done |
| `EventMaintenanceTest` | clock | M9 | `crates/shared`: `events::outbox::tests::the_purge_runs_at_the_next_3_17_utc`; `crates/shared/tests/outbox.rs::completed_rows_are_purged_after_seven_days` — done |
| `shared/id/UuidV7Test` | INT | M0 | `crates/shared/src/id.rs` — done; the BB part is `traps.rs::m6::expense_ids_are_uuid_v7` |
| `MoneyTest` | INT | M0 | `crates/shared`: `money::tests::*` (34 tests, e.g. `checked_add_across_currencies_is_a_currency_mismatch`, `rejects_scale_above_minor_units`) — done |
| `CursorTest` | INT | M4 | `crates/shared`: `paging::cursor::tests::{round_trips, a_cursor_java_produced_decodes, junk_is_an_invalid_cursor, the_token_is_url_safe}`; BB part in `audit.rs` (bad cursors) — done |
| `KeysetPagingTest` | INT | M4 | `crates/shared/tests/keyset_paging.rs::{pages_through_every_row_exactly_once_newest_first, rows_inserted_while_paging_cause_no_duplicates_or_loss, other_teams_rows_are_never_returned}` — done |
| `CappedInputStreamTest` | INT | M7 | `crates/receipts`: `web::tests::the_configured_cap_has_its_own_message_unless_spring_refused_first`; `crates/shared`: `web::extract::tests::a_body_over_the_limit_is_413_whether_declared_or_streamed` — done |
| `RateLimiterTest` | INT, clock | M10 | `crates/shared`: `ratelimit::tests::*` (11 tests, e.g. `allows_up_to_the_capacity_then_refuses_with_a_wait_time`, `concurrent_callers_never_get_more_than_the_capacity`) — done |
| `MdcUserFilterTest` | INT | M5 | `crates/shared`: `web::authentication::tests::a_valid_token_names_the_caller_on_any_path`; `crates/app`: `telemetry::tests::a_log_line_inside_a_request_carries_the_requests_ids`, `logging::tests::span_fields_appear_like_javas_mdc_and_inner_spans_win` — done |
| `MdcTeamFilterTest` | INT | M6 | `crates/shared`: `web::authentication::tests::team_scoped_paths_name_their_team_and_no_other_path_does` — done |
| `DevDataSeederTest` | INT | M5 | `crates/app/tests/demo_data.rs::{an_empty_database_gets_exactly_the_demo_data, a_second_start_adds_nothing, an_existing_account_keeps_its_id_and_password, only_local_without_test_and_without_keycloak_seeds}` — done |
| `S3ObjectStorageTest` | INT | M7 | `crates/shared`: `storage::tests::{put_open_move_and_delete, ensure_bucket_creates_a_missing_bucket_once, a_presigned_link_downloads_as_an_attachment_under_the_given_name, an_unreachable_store_is_an_error_without_the_key}` — done |
| `ExpenseTest` | INT | M0 | `crates/expenses`: `domain::expense::tests::*` (`creation`, `draft_only_operations`, `transitions`, `visibility`, `receipt_permissions`) — done |
| `ExpenseRepositoryTest` | INT | M2 | `crates/expenses`: `persistence::tests::*` (11 tests, e.g. `an_expense_round_trips`, `another_teams_expense_is_not_found`, `the_second_of_two_updates_from_the_same_version_conflicts`) — done |
| `UpdateExpenseTest` | INT | M3 | `crates/expenses`: `application::tests::{an_edit_publishes_the_names_of_the_changed_fields_in_javas_order, an_edit_that_changes_nothing_writes_nothing, only_the_fields_sent_change_and_the_version_moves}` — done |
| `CsvCellsTest` | INT | M8 | `crates/expenses`: `web::csv::tests::{formula_starts_get_a_quote_and_specials_are_quoted, the_header_is_javas}`; BB part in `expenses_batch_and_export.rs` — done |
| `LazyCsvInputStreamTest` | INT | M8 | `crates/expenses`: `web::export_tests::*` (7 tests, M11.7; the two `InputStream.read` tests have no counterpart, see the file) — done |
| `JwtKeysTest` | INT | M5 | `crates/identity`: `access_token::tests::{without_keys_local_and_test_generate_one_with_a_random_v4_kid, without_keys_elsewhere_the_app_does_not_start, the_kid_of_a_configured_key_is_its_rfc_7638_thumbprint, an_unreadable_key_is_refused_without_quoting_it, keys_may_be_bare_base64_without_the_pem_armour}` — done |
| `ExternalIdentityTest` (all but the two HTTP tests in `keycloak.rs` and `keycloak_unreachable.rs`) | INT | M11 | `crates/identity/src/application/provision/tests.rs` (provisioning, linking, the race of first requests) and `crates/identity/src/keycloak/tests.rs::a_blank_subject_is_refused` (`aMissingSubjectIsRefused`); the converter's mapping of `sub` is `keycloak.rs::a_keycloak_token_works_and_creates_our_user_on_first_use` — done |
| `LastOwnerConcurrencyTest` | INT | M6 | `crates/teams`: `application::concurrency_tests::*` (5 tests, e.g. `two_owners_leaving_at_the_same_time_leave_exactly_one_owner`) — done |
| `FileNamesTest` | INT | M7 | `crates/receipts`: `domain::tests::{download_names_keep_only_safe_characters_and_the_real_extension, long_download_names_are_shortened, download_names_never_contain_separators_quotes_or_control_characters}`; BB part in `receipts.rs` — done |
| `ClamAvScannerTest` | INT | M7 | `crates/receipts`: `scanner::tests::*` (7 tests against a fake clamd, e.g. `parses_a_found_reply_into_the_signature_name`, `a_scanner_that_never_answers_times_out`) — done |
| `EventRegistryRecoveryTest` | fault, INT | M9 | `crates/receipts`: `web::tests::a_scan_that_never_finished_is_done_after_a_restart`; `crates/shared/tests/outbox.rs::{a_dispatcher_killed_between_commit_and_delivery_delivers_exactly_once_after_a_restart, a_failed_delivery_stays_open_and_is_delivered_again_once_stuck}` — done |
| `ImportSkipListenerTest` | INT | M8 | `crates/imports`: `web::tests::{a_row_error_is_recorded_once_and_cut_at_500_characters, row_errors_carry_javas_codes_and_messages}` — done |
| `EveryEventReachesTheAuditTrailTest` | INT | M9 | `crates/audit`: `listener::tests::every_event_reaches_the_audit_trail_once_with_javas_name_and_details`; BB parts in `audit.rs` and `traps.rs::m9` — done |
| `AuditTrailTest` | INT | M9 | `crates/audit`: `listener::tests::{the_trail_cannot_be_updated_deleted_or_truncated, team_less_rows_are_never_part_of_a_teams_page}` — done |
| `AuditModuleTest` | INT | M9 | `crates/audit`: `listener::tests::{every_event_reaches_the_audit_trail_once_with_javas_name_and_details, every_audited_type_is_a_type_some_module_publishes_and_nothing_else_is_audited}` — done |

## Parts of Java tests whose other parts are parity tests

| Java test | Part that is not parity | Kind | Milestone | Ported in-process in |
| --- | --- | --- | --- | --- |
| `Api2BrokenAuthenticationTest.failedLoginsLockTheAccountEvenAgainstTheRightPassword` | the lock ends after 16 minutes | clock | M5 | `crates/identity`: `web::tests::the_address_locks_after_five_failures_until_the_period_is_over` — done |
| `Api2BrokenAuthenticationTest.accessTokensAreShortLivedAndTamperedOnesAreRefused` | a genuine token is refused after 11 minutes | clock | M5 | `crates/identity`: `access_token::tests::a_token_lives_ten_minutes_plus_thirty_seconds_of_skew` — done |
| `Api4UnrestrictedResourceConsumptionTest.aBurstOfRequestsFromOneUserGets429WithRetryAfterWhileOthersAreUnaffected` | the allowance is back after one minute | clock | M10 | `crates/shared`: `web::rate_limit::tests::a_user_over_the_budget_gets_429_with_retry_after_and_others_do_not`, `ratelimit::tests::gradual_limits_get_tokens_back_over_time` — done |
| `Api4UnrestrictedResourceConsumptionTest.theEndpointsThatNeedNoTokenAreLimitedByClientAddress` | the allowance is back after two minutes | clock | M10 | `crates/shared`: `web::rate_limit::tests::token_free_endpoints_are_limited_per_client_address`, `ratelimit::tests::gradual_limits_get_tokens_back_over_time` — done |
| `LogHygieneTest.noSecretOrPersonalDataReachesTheLogs` | none: it is a parity test (container log) | — | M10 | — |
| `AuthApiTest.verificationTokenExpiresAfter24Hours` | all of it | clock | M5 | `crates/identity`: `web::tests::{a_verification_token_expires_after_24_hours, a_verification_token_is_usable_until_the_instant_it_expires}` — done |
| `AuthApiTest.lockExpiresAfterFifteenMinutes` | all of it | clock | M5 | `crates/identity`: `web::tests::the_address_locks_after_five_failures_until_the_period_is_over` — done |
| `AuthApiTest.expiredTokenGets401` | all of it | clock | M5 | `crates/identity`: `access_token::tests::a_token_lives_ten_minutes_plus_thirty_seconds_of_skew` — done |
| `AuthApiTest.expiredRefreshTokenGets401` | all of it | clock | M5 | `crates/identity`: `web::tests::a_session_slides_14_days_from_each_refresh` — done |
| `AuthApiTest.verificationTokenIsStoredOnlyAsAHash` | that the stored hash differs from the issued token (the token is known only to Java's mailbox) | INT | M5 | `crates/identity`: `web::tests::a_new_account_is_unverified_lower_case_and_has_one_verification_for_24_hours`, `opaque_token::tests::the_hash_has_64_hex_characters_and_never_the_token`; the shape of the stored hash is in `auth.rs` — done |
| `TeamsApiTest.expiredInvitationCannotBeAcceptedAndNeedsNoScheduler` | all of it | clock | M6 | `crates/teams`: `web::tests::{an_expired_invitation_cannot_be_accepted_declined_or_revoked, an_invitation_one_microsecond_before_expiry_can_be_accepted}` — done |
| `TeamsApiTest.meOmitsExpiredAndRevokedInvitations` | the expired half | clock | M6 | `crates/teams`: `web::tests::me_omits_expired_and_answered_invitations` — done |
| `ReceiptsApiTest.anExeRenamedToPdfGets415` | the count of objects in the store | INT | M7 | `crates/receipts`: `web::tests::empty_and_unknown_files_are_refused_and_nothing_is_stored`; from outside, no receipt row exists — done |
| `ReceiptsApiTest.anEicarFileIsQuarantinedAndNeverServed` | the quarantined key was never presigned | INT | M7 | `crates/receipts`: `web::tests::{an_infected_file_is_quarantined_and_its_object_moved, pending_quarantined_and_unreachable_receipts_are_404}` — done |
| `ReceiptsApiTest.submitNeedsAnAcceptedReceiptAndNoPendingOne` | the scan run again in-process, then submit works | INT | M7 | `crates/receipts`: `web::tests::the_submit_rule_counts_accepted_and_pending_receipts_of_the_expense` — done |
| `ImportApiTest.aTenThousandRowImportWithThirtySevenBadRowsReportsExactlyThoseLines` | the captured `ImportCompleted` event | INT | M8 | `crates/imports`: `web::tests::an_import_creates_drafts_chunk_by_chunk_and_deletes_its_file` — done |
| `ImportApiTest.theLimitIsPerUserAndPerTeamAndResetsTomorrow` | the next day | clock | M8 | `crates/imports`: `web::tests::the_daily_limit_counts_the_utc_day_and_retry_after_runs_to_midnight` — done |
| `ImportApiTest.aFailedImportRestartsFromTheLastCommittedChunkWithoutDuplicates` | all of it | fault | M8 | `crates/imports`: `web::tests::a_failed_import_restarts_after_the_last_committed_chunk_without_duplicates` — done |
| `ImportApiTest.aCompletedImportCannotBeRestarted` | all of it | INT | M8 | `crates/imports`: `web::tests::a_completed_import_is_neither_restarted_nor_run_again` — done |
| `ImportApiTest.errorsAlreadyReportedBeforeAFailureAreNotDuplicatedOnRestart` | all of it | fault | M8 | `crates/imports`: `web::tests::errors_reported_before_a_failure_are_not_duplicated_on_restart` — done |
| `ImportApiTest.deliveringTheQueuedEventAgainDoesNotRunACompletedImportTwice` | all of it | INT | M8 | `crates/imports`: `web::tests::{a_completed_import_is_neither_restarted_nor_run_again, a_running_job_is_not_started_twice_in_one_process}` — done |
| `ExternalIdentityTest.aTokenIsRefusedCleanlyWhenTheIdentityProviderCannotBeReached` | none: it is a parity test (Keycloak stopped) | — | M11 | — |

## Parity traps that cannot be seen from outside

From §4 of the inventory.

| Trap | Why not parity | Milestone | Covered in-process in |
| --- | --- | --- | --- |
| §4.1 `INTERNAL_ERROR`: a 500 reveals only the trace id | needs a failing route | M3 | `crates/app`: `tests::a_panic_is_a_500_whose_trace_id_is_the_one_in_the_log_line` — done |
| §4.1 `IDEMPOTENCY_IN_PROGRESS` | needs two requests interleaved inside one transaction | M3 | `crates/expenses`: `application::tests::two_concurrent_requests_with_one_key_create_one_expense` — done |
| §4.4 the JWT validator allows 30 s of clock skew | clock | M5 | `crates/identity`: `access_token::tests::{a_token_lives_ten_minutes_plus_thirty_seconds_of_skew, not_before_is_honoured_with_the_same_skew}`; for Keycloak `keycloak::tests::expiry_and_not_before_have_thirty_seconds_of_skew` — done |
| §4.4 timing-equal login (a dummy hash for unknown emails) | no test measures time; Java asserts only equal responses, which `auth.rs` does | M5 | `crates/identity`: `web::tests::unknown_email_and_wrong_password_look_the_same`, `password::tests::matching_nothing_never_matches` — done |
| §4.4 rate-limit defaults (600 and 60 per minute) | config defaults | M1 | `crates/shared`: `config::tests::limits_default_to_javas_values` — done |
| §4.4 invitations expire after 7 days, computed on read | clock | M6 | `crates/teams`: `web::tests::an_invitation_is_pending_for_seven_days`, `domain::tests::a_pending_invitation_expires_at_its_expiry_instant` — done |
| §4.4 the idempotency record lives 24 hours | clock | M3 | `crates/expenses`: `application::tests::a_key_expires_after_24_hours` — done |
| §4.6 an invalid CORS allowlist refuses start-up | startup, not HTTP | M1 | `crates/app`: `config::tests::every_unusable_value_is_reported_at_once_by_variable` — done |
| §4.6 the `prod` profile refuses to start without its settings | startup, not HTTP | M1 | `crates/app/tests/startup.rs::prod_without_configuration_exits_non_zero_and_lists_every_missing_variable`; `crates/app`: `config::tests::prod_needs_…` (one per variable) — done |
