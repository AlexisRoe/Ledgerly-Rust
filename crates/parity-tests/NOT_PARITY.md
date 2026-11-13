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
| `Api7ServerSideRequestForgeryTest.onlyInfrastructureMayCallOut` | INT (ArchUnit) | M10 | `crates/app/tests/architecture.rs`: only infrastructure modules depend on HTTP and socket crates |
| `Api7ServerSideRequestForgeryTest.theRuleReallyFailsForAClassThatBreaksIt` | INT | M10 | `crates/app/tests/architecture.rs`: the check fails for a made-up dependency graph |
| `Api7ServerSideRequestForgeryTest.theSameCodeInAnInfrastructurePackageIsAllowed` | INT | M10 | `crates/app/tests/architecture.rs` |
| `Api8SecurityMisconfigurationTest.theAllowlistRefusesWildcardsPathsAndSloppyEntries` | INT | M1 | `crates/shared/src/config/mod.rs` (`is_valid_origin`) and `crates/app/src/config.rs` (startup refuses) — done |
| `Api8SecurityMisconfigurationTest.anUnexpectedFailureRevealsNothingButATraceId` | fault (a test-only route) | M3 | `crates/app` in-process router test with a failing handler |
| `Api9ImproperInventoryManagementTest.everyServedEndpointIsInTheSpecAndEveryDocumentedOneIsServed`, direction "served, therefore documented" | INT (route table) | M3 | `crates/app` test comparing the router with the spec; the other direction is `owasp_api9_inventory.rs` |
| `Api9ImproperInventoryManagementTest.theComparisonDetectsDriftInBothDirections` | INT | M3 | `crates/app`, the same test's helper |
| `Api9ImproperInventoryManagementTest.everyPathIsVersionedExceptTheKeyDiscoveryDocument` | INT | M5 | `crates/app`, the same route-table test |
| `LedgerlyApplicationTest` | INT | M1 | `crates/shared/src/config/mod.rs` (`limits_default_to_javas_values`) and `crates/app/src/logging.rs` — done |
| `ModularityTest` | INT | M1 | `crates/app/tests/architecture.rs` — done |
| `ModuleDocumentationTest` | INT | M9 | nothing: it writes Modulith diagrams; CONCEPT §3 holds the diagram |
| `ProductionEnvironmentCheckTest` | INT | M1 | `crates/app/src/config.rs` and `crates/app/tests/startup.rs` — done |
| `EventMaintenanceTest` | clock | M9 | `crates/shared` outbox tests with `FixedClock` |
| `shared/id/UuidV7Test` | INT | M0 | `crates/shared/src/id.rs` — done; the BB part is `traps.rs::m6::expense_ids_are_uuid_v7` |
| `MoneyTest` | INT | M0 | `crates/shared/src/money/` — done |
| `CursorTest` | INT | M4 | `crates/shared` cursor codec tests; BB part in `audit.rs` (bad cursors) |
| `KeysetPagingTest` | INT | M4 | `crates/shared` keyset query tests (testcontainers) |
| `CappedInputStreamTest` | INT | M7 | `crates/receipts` streaming byte counter tests |
| `RateLimiterTest` | INT, clock | M10 | `crates/shared` rate limiter tests with `FixedClock` |
| `MdcUserFilterTest` | INT | M5 | `crates/app` layer test with a captured span |
| `MdcTeamFilterTest` | INT | M6 | `crates/app` layer test with a captured span |
| `DevDataSeederTest` | INT | M5 | `crates/app` seeding test (testcontainers); the Bruno collection depends on it |
| `S3ObjectStorageTest` | INT | M7 | `crates/receipts` storage adapter tests against SeaweedFS |
| `ExpenseTest` | INT | M0 | `crates/expenses/src/domain/expense.rs` — done |
| `ExpenseRepositoryTest` | INT | M2 | `crates/expenses/tests/` persistence tests (testcontainers) |
| `UpdateExpenseTest` | INT | M3 | `crates/expenses` application tests (the event names only changed fields) |
| `CsvCellsTest` | INT | M8 | `crates/expenses` export cell tests; BB part in `expenses_batch_and_export.rs` |
| `LazyCsvInputStreamTest` | INT | M8 | `crates/expenses` streaming export tests |
| `JwtKeysTest` | INT | M5 | `crates/identity` key loading tests |
| `ExternalIdentityTest` (all but the two HTTP tests in `keycloak.rs` and `keycloak_unreachable.rs`) | INT | M11 | `crates/identity` just-in-time provisioning tests |
| `LastOwnerConcurrencyTest` | INT | M6 | `crates/teams` application tests (testcontainers) |
| `FileNamesTest` | INT | M7 | `crates/receipts` file name tests; BB part in `receipts.rs` |
| `ClamAvScannerTest` | INT | M7 | `crates/receipts` clamd client tests against a fake clamd |
| `EventRegistryRecoveryTest` | fault, INT | M9 | `crates/receipts` and `crates/shared` outbox tests |
| `ImportSkipListenerTest` | INT | M8 | `crates/imports` row error tests |
| `EveryEventReachesTheAuditTrailTest` | INT | M9 | `crates/audit` listener tests; BB parts in `audit.rs` and `traps.rs::m9` |
| `AuditTrailTest` | INT | M9 | `crates/audit` persistence tests (immutability by privileges and triggers) |
| `AuditModuleTest` | INT | M9 | `crates/audit` listener idempotency tests |

## Parts of Java tests whose other parts are parity tests

| Java test | Part that is not parity | Kind | Milestone | Ported in-process in |
| --- | --- | --- | --- | --- |
| `Api2BrokenAuthenticationTest.failedLoginsLockTheAccountEvenAgainstTheRightPassword` | the lock ends after 16 minutes | clock | M5 | `crates/identity` login tests with `FixedClock` |
| `Api2BrokenAuthenticationTest.accessTokensAreShortLivedAndTamperedOnesAreRefused` | a genuine token is refused after 11 minutes | clock | M5 | `crates/identity` token tests |
| `Api4UnrestrictedResourceConsumptionTest.aBurstOfRequestsFromOneUserGets429WithRetryAfterWhileOthersAreUnaffected` | the allowance is back after one minute | clock | M10 | `crates/shared` rate limiter tests |
| `Api4UnrestrictedResourceConsumptionTest.theEndpointsThatNeedNoTokenAreLimitedByClientAddress` | the allowance is back after two minutes | clock | M10 | `crates/shared` rate limiter tests |
| `LogHygieneTest.noSecretOrPersonalDataReachesTheLogs` | none: it is a parity test (container log) | — | M10 | — |
| `AuthApiTest.verificationTokenExpiresAfter24Hours` | all of it | clock | M5 | `crates/identity` verification tests |
| `AuthApiTest.lockExpiresAfterFifteenMinutes` | all of it | clock | M5 | `crates/identity` login tests |
| `AuthApiTest.expiredTokenGets401` | all of it | clock | M5 | `crates/identity` token tests |
| `AuthApiTest.expiredRefreshTokenGets401` | all of it | clock | M5 | `crates/identity` refresh tests |
| `AuthApiTest.verificationTokenIsStoredOnlyAsAHash` | that the stored hash differs from the issued token (the token is known only to Java's mailbox) | INT | M5 | `crates/identity` verification tests; the shape of the stored hash is in `auth.rs` |
| `TeamsApiTest.expiredInvitationCannotBeAcceptedAndNeedsNoScheduler` | all of it | clock | M6 | `crates/teams` invitation tests |
| `TeamsApiTest.meOmitsExpiredAndRevokedInvitations` | the expired half | clock | M6 | `crates/teams` `/me` tests |
| `ReceiptsApiTest.anExeRenamedToPdfGets415` | the count of objects in the store | INT | M7 | `crates/receipts` upload tests; from outside, no receipt row exists |
| `ReceiptsApiTest.anEicarFileIsQuarantinedAndNeverServed` | the quarantined key was never presigned | INT | M7 | `crates/receipts` download tests |
| `ReceiptsApiTest.submitNeedsAnAcceptedReceiptAndNoPendingOne` | the scan run again in-process, then submit works | INT | M7 | `crates/receipts` scan tests |
| `ImportApiTest.aTenThousandRowImportWithThirtySevenBadRowsReportsExactlyThoseLines` | the captured `ImportCompleted` event | INT | M8 | `crates/imports` runner tests |
| `ImportApiTest.theLimitIsPerUserAndPerTeamAndResetsTomorrow` | the next day | clock | M8 | `crates/imports` daily limit tests |
| `ImportApiTest.aFailedImportRestartsFromTheLastCommittedChunkWithoutDuplicates` | all of it | fault | M8 | `crates/imports` runner tests with a failing facade |
| `ImportApiTest.aCompletedImportCannotBeRestarted` | all of it | INT | M8 | `crates/imports` runner tests |
| `ImportApiTest.errorsAlreadyReportedBeforeAFailureAreNotDuplicatedOnRestart` | all of it | fault | M8 | `crates/imports` runner tests |
| `ImportApiTest.deliveringTheQueuedEventAgainDoesNotRunACompletedImportTwice` | all of it | INT | M8 | `crates/imports` listener tests |
| `ExternalIdentityTest.aTokenIsRefusedCleanlyWhenTheIdentityProviderCannotBeReached` | none: it is a parity test (Keycloak stopped) | — | M11 | — |

## Parity traps that cannot be seen from outside

From §4 of the inventory.

| Trap | Why not parity | Milestone | Covered in-process in |
| --- | --- | --- | --- |
| §4.1 `INTERNAL_ERROR`: a 500 reveals only the trace id | needs a failing route | M3 | `crates/app` (see `anUnexpectedFailureRevealsNothingButATraceId` above) |
| §4.1 `IDEMPOTENCY_IN_PROGRESS` | needs two requests interleaved inside one transaction | M3 | `crates/expenses` idempotency tests |
| §4.4 the JWT validator allows 30 s of clock skew | clock | M5 | `crates/identity` token tests |
| §4.4 timing-equal login (a dummy hash for unknown emails) | no test measures time; Java asserts only equal responses, which `auth.rs` does | M5 | `crates/identity` login tests (the dummy hash is verified) |
| §4.4 rate-limit defaults (600 and 60 per minute) | config defaults | M1 | `crates/shared/src/config/mod.rs` — done |
| §4.4 invitations expire after 7 days, computed on read | clock | M6 | `crates/teams` invitation tests |
| §4.4 the idempotency record lives 24 hours | clock | M3 | `crates/expenses` idempotency tests |
| §4.6 an invalid CORS allowlist refuses start-up | startup, not HTTP | M1 | `crates/app/src/config.rs` — done |
| §4.6 the `prod` profile refuses to start without its settings | startup, not HTTP | M1 | `crates/app/tests/startup.rs` — done |
