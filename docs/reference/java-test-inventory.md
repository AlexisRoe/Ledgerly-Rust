<!--
Paths: the Java source and tests are NOT in this repository (ADR 0007). `J/`, `JT/` and bare `src/main/java/...` or
`src/test/java/...` paths refer to Ledgerly-Java at the pinned commit: https://github.com/AlexisRoe/Ledgerly-Java/blob/e2a13909634808b347d30b3dec4c3ffee6741c02/
`R/` and `src/main/resources/...` are vendored under vendor/ledgerly-java/. `api/openapi.yaml` and `bruno/` are at
this repository's root.
-->

<!--
Extracted from Ledgerly-Java (commit in vendor/ledgerly-java/SOURCE) on 2026-11-12 by reading the tests; nothing was run.
Where this disagrees with the vendored tests, the tests win.
-->

# Ledgerly-Java test inventory for the Rust parity suite

Source: Ledgerly-Java at the pinned commit (read-only; nothing was built or run). All paths below are relative to
the Ledgerly-Java repository root unless they are absolute (see the note at the top for where each one lives). "Plan" = `docs/implementation-plan.md`. When a feature's milestone
was settled in the build history and not in the plan, the evidence is `docs/learning-log.md` (LL, with line numbers).

## 0. Legend and cross-cutting facts that decide reproducibility

**Columns**
- **Feat** is the earliest milestone whose *feature* the assertion needs.
- **BB setup** is the earliest milestone at which the test can be set up over HTTP alone. It differs from Feat when the
  Java test reached a state by SQL that HTTP can only reach through a later feature.
- **Kind**:
  - `BB`: reproducible with plain HTTP against the running app.
  - `BB+S`: reproducible over HTTP, but the Java test seeded the state with SQL. The HTTP setup needs the features in
    BB setup.
  - `BB+H`: needs a hook in the system under test. Hook types:
    - `clock`: time travel.
    - `mailbox`: the verification token.
    - `scanner`: a ClamAV outage or fake.
    - `cfg`: config overrides at start-up.
    - `profile`: the active profile.
    - `fault`: a route that throws.
    - `mgmt`: the management port.
  - `INT`: needs internals, such as DB inspection, log capture, ArchUnit, MeterRegistry, Java-level unit or bean
    calls. The note names the HTTP-observable part, if there is one.

**Facts that apply to almost every Java integration test** (they are the reason so many rows are `BB+S`/`BB+H`):
1. **Users and tokens are not made over HTTP.**
   - `TestUsers.create()` (`src/test/java/com/ledgerly/TestUsers.java`) INSERTs users with the hash
     `'not-a-real-hash'` and mints JWTs with `AccessTokenIssuer`.
   - Over HTTP: register, then log in. Unverified users *may* log in (`AuthApiTest.unverifiedUsersMayLogIn`).
2. **Teams, memberships, expenses (in any status) and receipts are INSERTed directly.**
   - The seeders are `TeamSeeder`, `ExpenseSeeder` and `ReceiptSeeder` (`src/test/java/com/ledgerly/*Seeder.java`).
   - Over HTTP:
     - A multi-role team needs `POST /teams`, an invitation, then `:accept` (plus a PATCH for the role).
     - `:accept` requires a **verified** email (403 `EMAIL_NOT_VERIFIED`).
     - The verification token reaches the outside world only through `LocalVerificationLogger` (local profile, log
       line `[local only] verify user {id} with POST /api/v1/auth/verify {"token":"..."}`,
       `src/main/java/com/ledgerly/identity/internal/application/LocalVerificationLogger.java:18-22`), or through the
       test bean `CapturedVerificationTokens`.
     - **So the Rust suite needs either a mailbox hook (test-only endpoint or log scraping) or the demo-data seed.**
   - `SUBMITTED`/`APPROVED`/`REJECTED`/`REIMBURSED` expenses need an `ACCEPTED` receipt first: upload, then a scan. The
     earliest BB setup is therefore **M7**.
3. **Clock.** `MutableClock` (`src/test/java/com/ledgerly/MutableClock.java`) is advanced in these tests:
   - lockout (+16 min)
   - token expiry (+11 min)
   - verification expiry (+25 h)
   - invitation expiry (+8 d)
   - refresh expiry (+15 d)
   - daily import allowance (+25 h)
   - rate-limit refill (+1 to 2 min)
4. **Scanner.** `FakeMalwareScanner` flags the EICAR string and has an `unavailable` switch
   (`src/test/java/com/ledgerly/receipts/internal/application/FakeMalwareScanner.java`). Real ClamAV detects EICAR,
   but "scanner unavailable" needs clamd stopped or a hook.
5. **Config in tests** (`src/test/resources/application-test.yaml`):
   - requests-per-minute 100000 and auth 100000.
   - `OwaspTestBase` overrides: `ledgerly.limits.requests-per-minute=250`,
     `ledgerly.security.allowed-origins=https://app.example.com`, `management.server.port=0`.
   - Api4 overrides: 30/min and auth 12/min.
   - BatchAndExport overrides: `max-export-rows=1200`.
6. **Profiles.** Tests run in the `test` profile. Swagger and `/docs` exist only in `local`. Demo data
   (`DevDataSeeder`) runs only in `local & !test`.

---

## 1. OWASP test classes (`src/test/java/com/ledgerly/owasp/`)

Shared base: `OwaspTestBase.java`.
- `newTeam()` seeds a team with OWNER, APPROVER, MEMBER and a second MEMBER ("colleague") by SQL.
- `expense(team, claimant, status)` INSERTs an expense row in any status.
- `PDF` is a 33-byte minimal PDF.

### Api1BrokenObjectLevelAuthorizationTest.java

| Method | Observable HTTP behaviour asserted | Kind | Feat | BB setup |
| --- | --- | --- | --- | --- |
| `aUserOfTeamAGets404ForTeamBsExpenseAndReceipt` | Team A's OWNER gets **404** for `GET /teams/B/expenses/{eB}` and `.../receipts/{rB}/download`, and also via `/teams/A/...` with B's ids. | BB+S | M7 | M7 (accepted receipt) |
| `aMemberGets404ForATeammatesExpense` | A MEMBER gets 404 for a colleague's expense. The claimant MEMBER and the APPROVER get 200. | BB+S | M6 | M6 (+mailbox) |
| `theListNeverShowsAnotherTeamsOrAnotherMembersExpenses` | A MEMBER calling `GET .../expenses?claimantId=<colleague>` gets `content` with exactly one item, their own. | BB+S | M6 (list M4) | M6 |
| `receiptsFollowTheirExpenseNotJustTheirOwnId` | Downloading receipt R through a sibling expense's path is 404. The colleague gets 404 on the right path. | BB+S | M7 | M7 |
| `theAuditTrailAndImportsOfOtherTeamsAreNotReachable` | A non-member gets 404 on `/teams/B/audit-events` and `/teams/B/imports/{random}`. | BB | M9 (imports M8) | M9 |
| `anOutsiderLearnsNothingAboutWhichTeamsExist` | `GET /teams/{real}/expenses` and `/teams/{random}/expenses` by an outsider return the same `code` (`NOT_FOUND`). | BB | M6 | M6 |

### Api2BrokenAuthenticationTest.java

| Method | Observable HTTP behaviour asserted | Kind | Feat | BB setup |
| --- | --- | --- | --- | --- |
| `aReplayedRefreshTokenRevokesTheWholeSessionFamily` | register 202, then login, then refresh 200. Replaying the old cookie gives 401, and the rotated cookie then also gives 401. Refresh sends `X-Requested-With: ledgerly` and `Cookie: refresh_token=...`. The verify step needs the mailbox but is optional (unverified users may log in). | BB (mailbox optional) | M5 | M5 |
| `anUnverifiedAccountCannotAcceptAnInvitation` | The OWNER invites an unverified user, whose `POST /invitations/{id}:accept` gets **403**, code `EMAIL_NOT_VERIFIED`. | BB | M6 | M6 |
| `failedLoginsLockTheAccountEvenAgainstTheRightPassword` | 5 wrong logins each give 401, then the right password also gives **401**. After +16 min it gives 200. | BB+H(clock) | M5 (bucket since M10) | M5 |
| `unknownEmailAndWrongPasswordAreIndistinguishable` | Same `status`, `detail` and `code` for a wrong password and an unknown email. | BB | M5 | M5 |
| `registeringAnExistingEmailLooksLikeRegisteringANewOne` | Re-registering an existing email gives the same status and an identical JSON body as a fresh email (202, `{message}`). | BB | M5 | M5 |
| `passwordsAreStoredAsArgon2idHashes` | DB: `users.password_hash` starts with `$argon2id$`. No HTTP assertion. | INT | M5 | n/a |
| `accessTokensAreShortLivedAndTamperedOnesAreRefused` | A forged signature on `GET /me` gives 401. A genuine token after +11 min gives 401. | BB (forged) / BB+H(clock) (expiry) | M5 (/me is M6) | M6 |

### Api3BrokenObjectPropertyLevelAuthorizationTest.java

| Method | Observable HTTP behaviour asserted | Kind | Feat | BB setup |
| --- | --- | --- | --- | --- |
| `patchWithStatusIsRejectedAndNothingChanges` | POST creates an expense (ETag). `PATCH {"status":"APPROVED"}` with `If-Match` gives **400**, and a later GET still shows `DRAFT`. | BB+S | M3 | M6 (team) |
| `claimantIdCannotBeSetOnCreateEditOrBatch` | An extra `claimantId` gives **400** on POST create, on PATCH (with If-Match) and on `POST expenses:batch` (an item with claimantId). Afterwards `claimantId` is unchanged. | BB+S | M8 (batch) | M8 |
| `roleAndIdentityFieldsCannotBeSetThroughTheTeamEndpoints` | `POST /teams {"name","owner"}` gives 400. `PATCH /teams/{t}/members/{u} {"role","teamId"}` gives 400. | BB+S | M6 | M6 |
| `responsesShowOnlyTheDocumentedFields` | The expense GET's keys are a subset of {id, teamId, claimantId, money, category, incurredOn, merchant, note, status, rejectionReasons}, without version, createdAt or updatedAt. `/me` keys are **exactly** {id, email, verified, teams, invitations}. | BB+S | M6 | M6 |

### Api4UnrestrictedResourceConsumptionTest.java (config: 30 req/min/user, 12 auth req/min/IP)

| Method | Observable HTTP behaviour asserted | Kind | Feat | BB setup |
| --- | --- | --- | --- | --- |
| `aHundredAndOneItemBatchIsA400` | 101 items to `expenses:batch` gives 400. | BB+S | M8 | M8 |
| `aPageOfMoreThanHundredIsRefusedNotClamped` | `expenses?size=101` gives 400. `audit-events?limit=101` (OWNER) gives 400. | BB+S | M9 (list M4) | M9 |
| `uploadsOverTenMegabytesAreA413` | A receipt upload of 10 MiB + 1 byte gives **413**. | BB+S | M7 | M7 |
| `jsonBodiesOverOneMegabyteAreA413BeforeTheyAreParsed` | A 2 MiB JSON body (`{"pad":...}`) to POST expenses gives **413**. The 413 comes before parsing; the field is unknown. | BB+S | M8 (JsonBodyLimitFilter, LL:97) | M8 |
| `aSixthImportOfTheDayIsA429` | 5 imports give 202. The 6th gives **429** with a `Retry-After` header. | BB+S | M8 | M8 |
| `aBurstOfRequestsFromOneUserGets429WithRetryAfterWhileOthersAreUnaffected` | 40 × `GET /me` gives **exactly 10** × 429. `Retry-After` is in [1,60]. `code=RATE_LIMITED`. Another user still gets 200. After +1 min the old user (with a new token) and a new user get 200. | BB+H(cfg 30/min, clock) | M10 | M10 |
| `theEndpointsThatNeedNoTokenAreLimitedByClientAddress` | 20 × bad login from one IP gives ≥8 × 429 with `Retry-After`. After +2 min, a login gives 401 again. | BB+H(cfg 12/min, clock) | M10 | M10 |

### Api5BrokenFunctionLevelAuthorizationTest.java

| Method | Observable HTTP behaviour asserted | Kind | Feat | BB setup |
| --- | --- | --- | --- | --- |
| `aMemberCallingApproveGets403` | A MEMBER `:approve` on **their own** SUBMITTED expense gives **403** `FORBIDDEN` (the role check comes before the self-dealing 409). | BB+S | M6 | M7 |
| `anApproverCannotReimburseAndAMemberCannotReject` | APPROVER `:reimburse` on an APPROVED expense gives 403. MEMBER `:reject {"reason":"no"}` on their own SUBMITTED expense gives 403. | BB+S | M6 | M7 |
| `teamAdministrationIsForOwnersOnly` | For APPROVER and MEMBER: POST invitations, PATCH members/{u}, DELETE members/{u} and GET audit-events all give **403**. The OWNER's invite gives **201**. | BB+S | M9 | M9 |
| `nobodyCanActAsAnotherRoleThroughHeadersOrTokens` | A MEMBER `:approve` on a colleague's SUBMITTED expense with `X-Debug-Role: OWNER` and `X-Debug-User-Id: <owner>` gives **404** (the headers are ignored). | BB+S | M6 | M7 |
| `aDemotionTakesEffectOnTheNextRequestWithTheSameToken` | The APPROVER's GET of a colleague's expense gives 200. The OWNER `PATCH role=MEMBER`. The same token's GET then gives **404**. | BB+S | M6 | M7 |

### Api6UnrestrictedAccessToSensitiveBusinessFlowsTest.java

| Method | Observable HTTP behaviour asserted | Kind | Feat | BB setup |
| --- | --- | --- | --- | --- |
| `anApproverCannotApproveOrRejectTheirOwnExpense` | `:approve` on one's own SUBMITTED expense gives **409** `EXPENSE_SELF_DEALING`. `:reject` gives 409. | BB+S | M6 | M7 |
| `anOwnerCannotReimburseTheirOwnExpense` | OWNER `:reimburse` on their own APPROVED expense gives 409 `EXPENSE_SELF_DEALING`. | BB+S | M6 | M7 |
| `somebodyElseCanStillDecideSoTheRuleIsNotJustABrokenWorkflow` | OWNER `:approve` on the APPROVER's SUBMITTED expense gives 200. | BB+S | M6 | M7 |
| `invitingARegisteredAndAnUnknownEmailLooksTheSame` | Both invites give the same status (201), the same JSON property-name list (same order), and the same `status` and `role` values. | BB+S | M6 | M6 |
| `importsAreCappedAtFivePerDay` | 5 imports give 202 and the 6th gives 429 (for a MEMBER). | BB+S | M8 | M8 |

### Api7ServerSideRequestForgeryTest.java (ArchUnit, no Spring context)

| Method | What it asserts | Kind | Feat |
| --- | --- | --- | --- |
| `onlyInfrastructureMayCallOut` (`@ArchTest` static field) | No class outside `..infrastructure..` or `..shared.storage..` depends on `java.net.http`, Spring web clients, Apache HC, OkHttp, the AWS SDK, `Socket`, `URL`, `URLConnection` or `Http(s)URLConnection`. | INT (ArchUnit). The Rust analogue is a static or dependency-boundary check, not HTTP. | M10 |
| `theRuleReallyFailsForAClassThatBreaksIt` | The rule fails on `archfixture/OutboundCallsFromTheWrongPlace`. | INT | M10 |
| `theSameCodeInAnInfrastructurePackageIsAllowed` | `ClamAvScanner` uses `Socket`, and the rule passes for the infrastructure and storage packages. | INT | M10 |

`archfixture/OutboundCallsFromTheWrongPlace.java` is not a test. It is a deliberately violating class (fetches a
user-supplied URL with `java.net.http.HttpClient`) that is used only by the test above.

### Api8SecurityMisconfigurationTest.java (allowed-origins = `https://app.example.com`)

| Method | Observable HTTP behaviour asserted | Kind | Feat | BB setup |
| --- | --- | --- | --- | --- |
| `everyResponseCarriesTheSecurityHeaders` | The headers are checked on a 200 `GET /me`, a 401 `GET /me` without a token, and an authenticated 404 on `GET /api/v1/nothing-here`:<br>`Strict-Transport-Security` contains `max-age=31536000` and `includeSubDomains`<br>`Content-Security-Policy` is **exactly** `default-src 'none'; frame-ancestors 'none'`<br>`X-Content-Type-Options: nosniff`<br>`X-Frame-Options: DENY`<br>`Referrer-Policy: no-referrer`<br>`Cache-Control` contains `no-store` | BB | M10 | M10 |
| `anAllowlistedOriginMayCallFromABrowser` | The preflight `OPTIONS /me` (Origin app, ACR-Method GET, ACR-Headers authorization) gives 200 with `Access-Control-Allow-Origin` = origin, `Access-Control-Allow-Credentials: true`, and `Access-Control-Allow-Headers` ∋ authorization. The actual GET gives 200, the same ACAO, and `Access-Control-Expose-Headers` ⊇ {ETag, Retry-After}. | BB+H(cfg) | M10 | M10 |
| `anyOtherOriginIsRefusedAndNeverGetsAWildcard` | The origins are `https://evil.example.com`, `https://app.example.com.evil.test`, `http://app.example.com` and `null`. For each, the preflight gives **403** with no ACAO, **and the actual authenticated GET also gives 403** with no ACAO. | BB+H(cfg) | M10 | M10 |
| `theAllowlistRefusesWildcardsPathsAndSloppyEntries` | Unit test: `new SecurityProperties(List.of(bad))` throws IAE for `*`, `https://*.example.com`, a trailing slash, a path, no scheme, `ftp://` and empty. The BB analogue is that the app refuses to start. | INT | M10 | n/a |
| `actuatorLivesOnItsOwnPortAndExposesOnlyHealthInfoAndPrometheus` | Management port: `/actuator/health`, `/actuator/health/liveness` and `/actuator/prometheus` give 200. `env`, `beans`, `heapdump`, `loggers`, `mappings` and `configprops` give ≠200. | BB+H(mgmt) | M10 (health M1) | M10 |
| `actuatorIsNotReachableOnThePublicPort` | Public `/actuator/{health,prometheus,env}` gives **401** without a token and **404** with one. | BB | M10 (401 since M5) | M10 |
| `swaggerUiIsNotServedOutsideLocalDevelopment` | Non-local profile: `/swagger-ui/index.html`, `/swagger-ui/openapi.yaml` and `/swagger-ui/swagger-ui-bundle.js` give 401 without a token and 404 with one. | BB+H(profile) | M5 (Swagger local-only M3) | M6 |
| `anUnexpectedFailureRevealsNothingButATraceId` | The test-only route `GET /api/v1/test-only/boom` gives 500 `application/problem+json` with `code=INTERNAL_ERROR`. The body has no exception text, class name, stack or "exception". Its keys are ⊆ {type, title, status, code, traceId, instance, detail}. | BB+H(fault) | M3 | M3 (needs fault route) |
| `notFoundAndBadMethodsAreProblemsNotWhitelabelPages` | An authenticated `GET /api/v1/does-not-exist` and `PUT /api/v1/me` give a `problem+json` content type. The body has no "whitelabel", "Tomcat" or "org.springframework". The status is **not** asserted. | BB | M3 | M6 |

### Api9ImproperInventoryManagementTest.java (profiles `local,test`)

| Method | What it asserts | Kind | Feat |
| --- | --- | --- | --- |
| `everyServedEndpointIsInTheSpecAndEveryDocumentedOneIsServed` | The Spring `RequestMappingHandlerMapping` set equals the spec's `METHOD /api/v1+path` set. JWKS is at the root because of its path-level `servers`. `/error` and `/test-only/` are excluded. There are >30 documented operations. | INT (route table). Partial BB: probe every spec operation and assert it is not a route-404 (`REQUEST_REJECTED`) or 405. Undocumented routes cannot be found black-box. | M3 |
| `theComparisonDetectsDriftInBothDirections` | Unit test of the `drift()` helper. | INT | M3 |
| `everyPathIsVersionedExceptTheKeyDiscoveryDocument` | Every mapped route is `/api/v1/...` or `/.well-known/jwks.json`. | INT | M5 |
| `theSpecTheApplicationServesIsTheFileInTheRepository` | `GET /swagger-ui/openapi.yaml` (no token, local profile) is **byte-equal** to `api/openapi.yaml` and contains `openapi: 3.1.0`. | BB+H(profile local) | M3 |

### Api10UnsafeConsumptionOfApisTest.java

Added when M1.10 ported the OWASP tests: the class exists at the pinned commit but was missing from this list.

| Method | Observable HTTP behaviour asserted | Kind | Feat | BB setup |
| --- | --- | --- | --- | --- |
| `aFormulaInTheDataIsEscapedInTheExport` | A merchant `=HYPERLINK(...)` and a note `@SUM(1+1)` (set by SQL) come out of `expenses.csv` prefixed with `'` and RFC 4180-quoted; no data line contains `,=` or `,@`. | BB+S | M8 | M8 |
| `aFormulaThatEntersThroughAnImportIsStoredAsTypedAndEscapedOnTheWayOut` | An imported merchant `=cmd\|' /C calc'!A0` and note `+1+1` are stored verbatim (DB) and exported as `'=cmd...` and `,'+1+1`. | BB+S | M8 | M8 |
| `aFileIsNeverAcceptedUntilTheScannerHasClearlySaidSo` | With the scanner unavailable, an upload stays `PENDING_SCAN` and its download is 404. | BB+H(scanner) | M7 | M7 |
| `anInfectedFileIsQuarantinedAndNeverServed` | An EICAR upload becomes `QUARANTINED` within 20 s; download is 404 for the claimant and the approver. | BB+S | M7 | M7 |
| `theClaimedFileTypeAndNameAreNeverTrusted` | An EXE named `invoice.pdf` and HTML declared as `image/png` both give 415. | BB+S | M7 | M7 |
| `aCsvThatIsReallyBinaryIsRefused` | An EXE uploaded as an import gives 415; the export still gives 200. | BB+S | M8 | M8 |

### LogHygieneTest.java

| Method | What it asserts | Kind | Feat |
| --- | --- | --- | --- |
| `noSecretOrPersonalDataReachesTheLogs` | **Flow:** register (202), verify (204, mailbox), wrong login, login (200), refresh, refresh replay, logout, invite, create expense, receipt upload, batch, import (await COMPLETED), export, a 404 GET by a colleague. It then waits until ≥5 `audit_events` rows exist (DB).<br>**Captured stdout must not contain:** both emails, `@private-domain.example`, both passwords, the verification token, both access tokens, both refresh cookies, the owner's token, `"Bearer "`, the receipt content marker, the receipt filename, the merchant marker, the note marker, `password_hash`, `argon2id`.<br>**Logs must contain** the team id (MDC). | INT (log capture plus DB). It can be ported as "capture container stdout and grep". | M10 (needs M9 audit and M8 import) |

### MetricsTest.java

| Method | What it asserts | Kind | Feat |
| --- | --- | --- | --- |
| `successfulTransitionsAreCountedByAction` | HTTP: `:submit` gives 200, `:approve` gives 200, and a second `:approve` gives 409. The registry counter `ledgerly.expenses.transitions{action=submit\|approve}` grows by exactly +1 each, and the refused call is not counted. | INT (MeterRegistry). BB via a Prometheus scrape diff. | M10 |
| `quarantinedReceiptsAreCounted` | Uploading an EICAR file raises `ledgerly.receipts.quarantined` by exactly +1. | INT, or BB via scrape plus EICAR | M10 |
| `skippedImportRowsAreCounted` | An import with an `abc` amount row and a `ZZZ` currency row raises `ledgerly.imports.rows.skipped` by +2. The HTTP `GET imports/{id}` gives `skippedRows == 2`. | INT, or BB via scrape | M10 |
| `prometheusExposesThemOnTheManagementPortOnly` | Management `/actuator/prometheus` gives 200. The body contains `ledgerly_expenses_transitions_total{action="submit"`, `jvm_memory_used_bytes` and `http_server_requests`, and no user email or team id. Public `/actuator/prometheus` with a token gives 404. | BB+H(mgmt) | M10 |

---

## 2. All other test classes under `src/test/java` (grouped by module)

Format: path → what it covers → Feat milestone → Kind (with the HTTP-observable highlights where they matter).

### Root `com.ledgerly`
- `src/test/java/com/ledgerly/ActuatorHealthTest.java`
  - Covers: management port `/actuator/health` gives 200 with a body containing `"status":"UP"`.
  - **M1**. BB+H(mgmt).
- `src/test/java/com/ledgerly/LedgerlyApplicationTest.java`
  - Covers: the context loads with the default `LedgerlyLimits` (page 100, batch 100, upload 10MB, rows 10000,
    imports/day 5), and the log lines are JSON with a `message` field (ECS).
  - **M1**. INT (bean plus log capture).
- `src/test/java/com/ledgerly/ModularityTest.java`
  - Covers: Spring Modulith `ApplicationModules.verify()`.
  - **M1**. INT.
- `src/test/java/com/ledgerly/ModuleDocumentationTest.java`
  - Covers: writes the Modulith diagrams to `docs/modules/`.
  - **M9**. INT (n/a for Rust).
- `src/test/java/com/ledgerly/SwaggerUiProfileTest.java`
  - Local profile:
    - `/swagger-ui/index.html`, `/swagger-ui/openapi.yaml` and `/swagger-ui/swagger-ui-bundle.js` give 200.
    - `GET /docs` redirects to a page containing "swagger".
    - `POST /docs` gives 401.
  - Non-local profile: `/swagger-ui/index.html` and `/docs` give 401.
  - **M3** (401s need M5). BB+H(profile).
- Helpers, which are not tests (each one is an internal shortcut the Rust suite must replace):
  - `TestUsers.java`: SQL users plus minted JWTs.
  - `TeamSeeder.java`: SQL teams and memberships, plus `remove`.
  - `ExpenseSeeder.java`: SQL expense in any status, merchant `'Seed'`, version 0.
  - `ReceiptSeeder.java`: SQL receipt in any scan status.
  - `MutableClock.java` and `TestClockConfiguration.java`: `@Primary` clock.
  - `TestcontainersConfiguration.java`: Postgres.

### security
- `src/test/java/com/ledgerly/security/RoleMatrixTest.java`
  - Covers: the 16 team-scoped endpoints × {no token, non-member, MEMBER, APPROVER, OWNER}, 80 cells. The expected
    statuses are a table in the source, lines 103-171:
    - Non-member: 404 everywhere. No token: 401 everywhere.
    - A MEMBER on a colleague's expense gets 404.
    - APPROVER/OWNER get **403** (not 404) for PATCH, DELETE, `:submit`, `:withdraw` and `:reopen` on a colleague's
      expense. Reimburse is 403 for APPROVER and 200 for OWNER.
    - Invitations and members admin: MEMBER/APPROVER 403, OWNER 201/204/200.
    - `:leave` gives 204 for every role, because a second OWNER is seeded.
    - The PATCH and DELETE cells send `If-Match: "0"`.
  - **M6**. BB+S (the SUBMITTED/APPROVED/REJECTED fixtures need M7 over HTTP; the invitation row is seeded by SQL).

### shared
- `shared/money/MoneyTest.java` (all paths in this group are under `src/test/java/com/ledgerly/`)
  - Covers the Money rules:
    - null, zero and negative amounts are rejected.
    - The scale must not exceed the currency's minor units (EUR 10.001 ✗, JPY 1.5 ✗, KWD 1.001 ✓).
    - Extra trailing zeros are fine, and the scale is normalised.
    - XAU, XTS and XXX are rejected.
    - plus and compareTo across currencies throw.
  - **M0**. INT (visible over HTTP only through create/batch/import validation).
- `shared/id/UuidV7Test.java`
  - Covers: UUIDv7 version and variant, embedded timestamp, time ordering. Expense ids use it.
  - **M2**. INT.
- `shared/paging/CursorTest.java`
  - Covers: the cursor round-trips and is URL-safe. Junk, and valid base64 of the wrong JSON, both give a 400
    exception.
  - **M4**. INT (BB via `audit-events?after=junk`, M9).
- `shared/paging/KeysetPagingTest.java`
  - Covers: keyset paging over `expenses`, newest first, exactly once; the last page has no cursor; an empty team;
    rows inserted mid-paging; team isolation.
  - **M4**. INT.
- `shared/io/CappedInputStreamTest.java`
  - Covers: exactly the limit is OK, +1 byte fails, and single-byte reads are counted.
  - **M7**. INT.
- `shared/ratelimit/RateLimiterTest.java`
  - Covers the Bucket4j wrapper: capacity, wait time, gradual vs interval refill, reset, independent scopes,
    concurrency.
  - **M10**. INT.
- `shared/security/MdcUserFilterTest.java`
  - Covers: `userId` in the MDC only for the duration of the request; anonymous requests get none.
  - **M5**. INT.
- `shared/security/MdcTeamFilterTest.java`
  - Covers: team-scoped paths put `teamId` in the MDC; other paths and non-UUIDs get none.
  - **M6**. INT.
- `shared/config/EventMaintenanceTest.java`
  - Covers: stuck events are redelivered after ≥2 min, and completed ones are kept 7 days.
  - **M9**. INT.
- `shared/config/ProductionEnvironmentCheckTest.java`
  - Covers the `prod` profile's fail-fast:
    - Every missing DB, key, S3 and ClamAV setting is listed at once, and blank values count as missing.
    - The `keycloak` profile needs the issuer but no keys.
    - The message is actionable, not a stack trace.
  - **M11**. INT (BB analogue: start-up exit code plus stderr).
- `shared/seed/DevDataSeederTest.java`
  - Covers the local demo data: the team, alice/bob/carol/dave, expenses in every status, one invitation, the demo
    password works, and the seed is idempotent.
  - **Not in the plan** (it is the Bruno prerequisite). INT.
- `shared/storage/S3ObjectStorageTest.java`
  - Covers the real SeaweedFS adapter: put/open/move/delete. A presigned GET has `X-Amz-Expires=300` and
    `X-Amz-Signature`, and downloads as an attachment under the given name. Observations are tagged without keys.
  - **M7/M8** (observations M10). INT.
- `shared/storage/FileSystemObjectStorage.java`: helper. A temp-dir object store that records presigned links. Not a
  test.
- `shared/web/ChunkedBodyLimitTest.java`
  - Covers: a 2 MiB **chunked** `POST /api/v1/auth/login` gives 413 with a body containing `PAYLOAD_TOO_LARGE`. A
    small chunked login gives 401.
  - **M8**. BB.

### identity
- `src/test/java/com/ledgerly/identity/internal/web/AuthApiTest.java`: the full auth surface. **M5** (`/.well-known`
  and refresh in M5). Mostly BB; the items marked INT, clock or mailbox below need hooks or internals.
  - **Register:**
    - 202 with an identical body for new, taken and UPPERCASE-taken emails. INT: there is one DB row.
    - Password `short` gives 400, a non-email gives 400, and a 129-character password gives 400.
    - The argon2id hash. INT.
  - **Verify:**
    - 204 the first time and 400 on reuse. INT: the `verified` flag.
    - The token is stored as a 64-hex hash. INT.
    - After +25 h it gives 400. Clock.
    - A guessed 43-character token gives 400.
  - **Login:**
    - The body has `tokenType=Bearer` and `expiresIn=600`.
    - `Set-Cookie` starts with `refresh_token=` and has `HttpOnly`, `Secure`, `SameSite=Strict` and
      `Path=/api/v1/auth`. The body does not contain the cookie value.
    - **`Cache-Control` is exactly `no-store`**.
    - Unverified users may log in.
    - Wrong vs unknown: the same 401, `code`, `detail` and `title`, and **no Set-Cookie on the 401**.
    - 5 failures lock the account, and the locked 401's `detail` does not mention "lock". The lock lasts 15 min
      (clock).
    - A success resets the failure counter: 4 failures, a success, 4 failures, a success.
  - **JWT:**
    - ES256, with a non-blank `kid`. The claims are **exactly** {sub, iat, exp, jti}, `sub` = user id, and
      exp − iat = 600 s.
    - The JWKS `keys[0]` has `kty=EC` and `crv=P-256`, no `d`, and its `kid` matches the token.
  - **Token checks:**
    - A valid token on a random team gives 404. No token gives 401 with `code=UNAUTHENTICATED`,
      **`WWW-Authenticate` starting with `Bearer`**, and `problem+json`.
    - These all give 401: expired (minted plus clock), tampered payload, `alg:none`, the wrong EC key, the HS256
      confusion attack (signed with the JWKS `x` coordinate), and garbage (`abc`, `a.b.c`, `....`, `Bearer`).
  - **Refresh:**
    - Rotates the cookie and returns a new access token, and the new cookie works.
    - Replay revokes the family. INT: the refresh token is stored hashed.
    - A missing or wrong `X-Requested-With` (`XMLHttpRequest`) gives **400** and does not consume the token. A
      missing cookie gives 401. An unknown cookie gives 401.
    - After +15 d it gives 401 (clock; the TTL is 14 d).
  - **Logout:**
    - 204 with `Set-Cookie` containing `Max-Age=0` and `Path=/api/v1/auth`, and the family is dead.
    - Logout with no cookie but with the header gives 204. Without the header it gives 400.
  - **Log hygiene:** INT (log capture).
- `src/test/java/com/ledgerly/identity/internal/infrastructure/JwtKeysTest.java`
  - Covers: a key is generated when allowed, start-up fails fast when it is not allowed and none is configured, PEM
    keys load with a stable kid, and garbage keys are rejected.
  - **M5/M11**. INT.
- `src/test/java/com/ledgerly/identity/internal/keycloak/ExternalIdentityTest.java` (profile `keycloak,test`)
  - Covers JIT user provisioning from hand-made IdP tokens: a stable mapping from external subject to user, no email
    in the audit, verified-email linking only, no takeover, and concurrent first requests create one user.
  - HTTP parts:
    - In the keycloak profile, Ledgerly's own `/auth/*` endpoints and JWKS give **404**.
    - An unreachable IdP gives 401 `problem+json` with `UNAUTHENTICATED` and no "Exception".
  - **M11**. Mostly INT.
- `src/test/java/com/ledgerly/identity/internal/keycloak/KeycloakEndToEndTest.java` (a real Keycloak 26.8 container)
  - Covers:
    - A Keycloak password-grant token works and creates the user on first use.
    - The rest of the API works unchanged with those tokens.
    - An unverified email cannot accept invitations.
    - A token for another audience gives 401. Tampered and garbage tokens give 401.
    - Linking to an existing local account.
    - `/auth/login`, `/auth/register` and `/auth/refresh` give 404.
  - **M11**. BB+H(profile keycloak plus Keycloak).
- `src/test/java/com/ledgerly/identity/internal/application/CapturedVerificationTokens.java`: helper (the mailbox).
  Not a test.

### teams
- `src/test/java/com/ledgerly/teams/internal/web/TeamsApiTest.java`: **M6**, BB+S (mailbox for verified users; clock
  for expiry; INT for a DB count).
  - **Creating a team:**
    - 201 with Location `/api/v1/teams/{id}`, and the creator is OWNER (via `/me`).
    - The name `""`, a 101-character name and `{}` give 400. No token gives 401.
  - **Inviting:**
    - The response is identical for registered and unknown emails, and also for the owner's own (already-member)
      address.
    - A bad email gives 400 and role `BOSS` gives 400.
  - **Re-invite:**
    - Re-inviting the same email, case-insensitively, replaces the pending invitation (INT: one PENDING row).
    - The old id's accept gives 409. The new one's accept gives 200 with the new role.
  - **Accept:**
    - 200 `{teamId, role}`, and the email match is case-insensitive.
    - An unverified user gets 403 `EMAIL_NOT_VERIFIED`. A stranger gets 404 on accept and decline. A random id gives
      404.
    - +8 days gives 409 `INVITATION_EXPIRED` (clock).
    - Accepting twice gives 409 `INVITATION_NOT_PENDING`. An already-member gives 409 `ALREADY_MEMBER`.
  - **Decline and revoke:**
    - Decline gives 204, then accept gives 409 and a second decline gives 409.
    - Revoke gives 204, then accept gives 409 and a second revoke gives 409.
    - Revoking another team's invitation through my team gives 404.
  - **Members:**
    - PATCH role gives 200. An unknown member gives 404 and role `BOSS` gives 400.
    - With a single OWNER, demote, remove and leave each give 409 `LAST_OWNER`. With two OWNERs, demote gives 200 and
      leave gives 204.
    - A removed member then gets 404 on the list. A member can leave (204), and a second leave gives 404.
    - A demotion shows in `/me` immediately.
  - **`/me`:**
    - Lists roles per team and pending invitations (`id`, `teamName`, `role`) for a verified email. After accept, the
      invitations are empty and the team shows.
    - An unverified user gets no invitations. Revoked and expired invitations are omitted.
    - No token gives 401.
  - **Edge case:**
    - A removed claimant gets 404 on `:withdraw` and GET.
    - An approver can still `:approve` (200), and the response keeps the old `claimantId`.
- `src/test/java/com/ledgerly/teams/internal/application/LastOwnerConcurrencyTest.java`
  - Covers: two OWNERs who demote, remove or leave each other at the same time always leave exactly one OWNER.
  - **M6**. INT, use-case level (BB-approximable with parallel HTTP).

### expenses
- `src/test/java/com/ledgerly/expenses/internal/domain/ExpenseTest.java`
  - Covers the state machine: 5 statuses × 6 actions with exactly 6 legal pairs, role rules, self-dealing, submit
    needs ≥1 accepted and 0 pending receipts, the reject reason is required and capped, all rejections are kept,
    draft-only edit/delete/receipts by the claimant, and visibility (a removed claimant still sees their own expenses
    at domain level).
  - **M0**. INT (all of it surfaces through the transition endpoints).
- `src/test/java/com/ledgerly/expenses/internal/infrastructure/ExpenseRepositoryTest.java`
  - Covers: round trip, team scoping, unknown id is empty, and an optimistic-lock failure on a concurrent save.
  - **M2**. INT.
- `src/test/java/com/ledgerly/expenses/internal/application/UpdateExpenseTest.java`
  - Covers partial edits:
    - Only the fields that were sent change, and the `Edited` event names only the fields that really changed
      (names, never values).
    - A blank note clears the note, and sending the same note is no change.
    - A stale version, a non-claimant OWNER, an outsider or a future date changes nothing and emits nothing.
  - **M3** (event content M9). INT (the blank-note clearing is BB-visible).
- `src/test/java/com/ledgerly/expenses/internal/web/ExpensesApiTest.java`: **M3**. BB+S, with real users from M6.
  - **Create:**
    - 201, Location `/api/v1/teams/{t}/expenses/{id}`, **`ETag: "0"`**, status DRAFT, `amount` "12.50", and
      `claimantId` = the caller.
  - **GET:**
    - The ETag equals the create ETag.
  - **PATCH:**
    - 200, the merchant and note change, and **`ETag "1"`**.
  - **DELETE:**
    - 204, then GET gives 404.
  - **Workflows:**
    - Full workflow: approve gives APPROVED and reimburse gives REIMBURSED.
    - reject with `{"reason":"blurry"}` gives REJECTED and `rejectionReasons[0]`. Then reopen gives DRAFT, submit, and
      withdraw gives DRAFT.
  - **Errors:**
    - PATCH with status or claimantId gives 400 **`MALFORMED_REQUEST`**.
    - A MEMBER's approve gives 403 `FORBIDDEN`.
    - Self-approve gives 409 `EXPENSE_SELF_DEALING`.
    - Approving a DRAFT gives 409 `EXPENSE_INVALID_TRANSITION` with `type=https://ledgerly.dev/problems/invalid-transition`.
    - `12.345` EUR gives 400 with `errors[0].field="money"`.
    - Date +2 days gives 400 with `errors[0].field="incurredOn"`.
    - Missing fields give 400 `VALIDATION_FAILED` with ≥2 `errors`.
    - `{not json` gives 400 `MALFORMED_REQUEST`.
  - **Preconditions:**
    - PATCH or DELETE without `If-Match` gives **428**.
    - A stale `"99"` gives **412 `PRECONDITION_FAILED`**, and `If-Match: garbage` also gives 412.
    - A second PATCH with the old ETag gives 412.
    - PATCH or DELETE of a non-draft gives 409 `EXPENSE_NOT_DRAFT`.
  - **Visibility:**
    - Another MEMBER gets 404 on GET, PATCH and `:submit`.
    - A non-teammate gets 404 on GET and POST.
    - Cross-team gives 404, and the APPROVER sees a MEMBER's expense.
  - **Other:**
    - No token gives 401 `UNAUTHENTICATED`.
    - Submit without a receipt gives 422 `EXPENSE_RULE_VIOLATED`.
    - A reject with reason `""` gives 400.
    - **An unknown path gives 404 with code `REQUEST_REJECTED`**.
  - **Idempotency:**
    - The same key and same body give 201 with the same id.
    - A different body gives 422 `IDEMPOTENCY_KEY_REUSED`.
    - The same key from another user is independent.
    - Without a key, every POST creates a new expense.
- `src/test/java/com/ledgerly/expenses/internal/web/ExpensesListApiTest.java`: **M4**. BB+S (250 rows are seeded by
  SQL with chosen `created_at` values).
  - **Paging:**
    - 250 rows over 3 pages of 100, each visited exactly once, with page meta `{size, number, totalElements,
      totalPages}`.
    - A page past the end gives 200 with an empty list.
    - `size=101` gives 400 `VALIDATION_FAILED`. `size=0`, `page=-1` and `size=abc` give 400.
  - **Sorting:**
    - The default sort is `incurredOn` desc. `incurredOn,asc` and `createdAt,desc` work.
    - `amount` without `currency` gives 400 with `errors[0].field="sort"`. `amount` with a currency sorts
      numerically.
    - These sort values give 400: `password`, `claimantId`, `id.value,asc`, `incurredOn,sideways`, `money.amount`,
      `a,b,c`.
  - **Filters:**
    - status, category, currency, claimantId and the date range combine. The date range is inclusive.
    - These give 400: `status=BOGUS`, **`currency=ZZZ`**, `currency=eur`, from > to, and `incurredFrom=yesterday`.
  - **Visibility:**
    - A MEMBER sees only their own expenses even with `claimantId=<other>`, and `totalElements` counts their own only.
    - APPROVER and OWNER see everyone. A non-teammate gets 404. Another team's rows never show.
- `src/test/java/com/ledgerly/expenses/internal/web/BatchAndExportApiTest.java`: **M8**. BB+S (SQL seeding; the cap
  test needs cfg `max-export-rows=1200` or 10,001 rows).
  - **Batch:**
    - 100 items give 200 with `created=100`, `rejected=0` and 100 distinct ids, all `CREATED`.
    - 101 items give 400 and nothing is created. `items:[]` gives 400.
    - Mixed items give 200 with `created=3`, `rejected=5`, and a `results[i].index` for each item. The
      `problem.field` values are:
      - `money` for 12.345, `abc` and `ZZZ`.
      - `incurredOn` for a future date.
      - **`merchant` for a `"   "` merchant**.
      - Rejected items have no `id`.
    - `atomic=true` with one bad item gives 422 `BATCH_REJECTED`, `errors[0].field="items[2].incurredOn"`, and
      nothing is created. Several problems give `["items[0].money","items[2].merchant"]`. All valid gives 200.
    - An item with `status` or `claimantId` gives 400 for the whole request.
    - A non-teammate gets 404 and no token gives 401.
    - A 2 MiB body gives 413 `PAYLOAD_TOO_LARGE`, **even without a token**, which is 413 rather than 401.
  - **Export:**
    - 200 with `text/csv` and `Content-Disposition` attachment `expenses.csv`.
    - `X-Total-Count` and `X-Truncated: false`.
    - The header row is exactly `id,incurredOn,merchant,amount,currency,category,status,claimantId,note`.
    - **CRLF line endings**, and rows contain `,10.00,EUR,MEALS,DRAFT,<uuid>`.
    - Formula neutralising: `'=`, `'+`, `'-`, `'@` and `'\t` prefixes, RFC 4180 quoting (`"Smith, Jones & Co"`).
    - A MEMBER exports only their own rows even with `claimantId`. The list's filters apply.
    - 1,150 rows stream without repeats or gaps.
    - With the cap at 1,200, 1,300 matching rows give `X-Total-Count: 1300`, `X-Truncated: true` and 1,200 rows.
    - Nothing matching gives just the header.
    - An outsider gets 404 and no token gives 401.
    - `currency=ZZZ`, `currency=eur`, `status=BOGUS` and from > to give 400.
- `src/test/java/com/ledgerly/expenses/internal/web/CsvCellsTest.java`
  - Covers cell escaping: a formula prefix gets a quote prefix; tab and CR are neutralised; empty stays empty; RFC 4180
    quoting; both together; rows end in CRLF.
  - **M8**. INT (BB via export).
- `src/test/java/com/ledgerly/expenses/internal/web/LazyCsvInputStreamTest.java`
  - Covers the streaming export: header first, lazy page fetch, ordering, byte-wise and bulk reads give the same
    bytes, multiline notes stay one cell, a missing note is an empty last cell, and the row count.
  - **M8**. INT.

### receipts
- `src/test/java/com/ledgerly/receipts/internal/web/ReceiptsApiTest.java`: **M7**. BB+S, with hooks marked.
  - **Upload:**
    - 202 with `status=PENDING_SCAN`, `detectedType`, `sizeBytes` and `possibleDuplicate=false`.
    - **Location = the list path `.../expenses/{e}/receipts`**.
    - It becomes ACCEPTED (poll the list).
  - **Download:**
    - 200 `{url, expiresAt}`. In the test, `url` starts with `https://storage.test/receipts/`; that host belongs to
      the fake store.
    - `expiresAt` ≤ now + 5 min, and `Cache-Control: no-store`.
  - **Types:**
    - JPEG and PNG are accepted.
    - A PDF declared as `image/png` gives `detectedType=application/pdf`.
    - An EXE named `.pdf` gives 415 `UNSUPPORTED_MEDIA_TYPE`, the list stays empty and no object is stored (INT).
    - Text `hello` gives 415, and HTML declared as png gives 415.
    - An empty file gives 400.
  - **Size and form:**
    - 10 MiB + 1 gives 413 `PAYLOAD_TOO_LARGE`. Exactly 10 MiB gives 202.
    - A missing `file` part gives 400.
  - **Scanning:**
    - EICAR gives QUARANTINED, and download is 404 for the claimant and the approver. INT: the key starts with
      `quarantine/` and is never presigned.
    - Scanner down (hook) leaves PENDING_SCAN and download gives 404.
  - **Submit rule:**
    - No receipt gives 422. Pending gives 422 `EXPENSE_RULE_VIOLATED`.
    - Then a rescan (INT call to `ScanReceipt.scan`) makes it ACCEPTED, and submit gives 200 SUBMITTED.
    - A quarantined receipt does not count (422).
  - **Filenames:**
    - The filename `../../etc/passwd` is kept verbatim as `originalName`. INT: the key is `receipts/<uuid>`, and the
      presigned download name is `passwd.pdf` with a TTL ≤ 5 min.
    - **`nice\tname.pdf` gives `originalName` `nicename.pdf`**.
  - **Duplicates:**
    - The same bytes twice on one expense give 409 `RECEIPT_DUPLICATE`.
    - On a sibling expense: 202 with `possibleDuplicate=true`. In another team: `false`.
    - Two parallel identical uploads give {202, 409}.
  - **Authorization:**
    - Uploading to a SUBMITTED expense gives 409 `EXPENSE_NOT_DRAFT`.
    - Uploads by the approver give **403**, by a colleague **404**, by an outsider 404, and with no token 401.
    - Other team's path gives 404. A receipt id under the wrong expense gives 404 on download and DELETE.
    - The list and download follow the expense's visibility: approver 200, colleague 404, outsider 404, none 401.
  - **Delete:**
    - The claimant's delete of a draft's receipt gives 204, the list is empty, and a second delete gives 404 (INT:
      the object is gone).
    - After the expense is SUBMITTED (SQL), the claimant's delete gives 409 and the approver's gives 403.
    - Deleting the expense needs If-Match: without it 428, with it 204, and then its receipts disappear
      asynchronously (INT check).
- `src/test/java/com/ledgerly/receipts/internal/application/FileNamesTest.java`
  - Covers: the sanitised download name keeps only safe characters and the real extension, has no separators, quotes
    or control characters, and long names are shortened.
  - **M7**. INT.
- `src/test/java/com/ledgerly/receipts/internal/infrastructure/ClamAvScannerTest.java`
  - Covers the INSTREAM framing against a fake clamd: chunked, zero terminator, NUL reply. FOUND gives the signature
    name. ERROR or unreachable gives an IOException. Anything but the two exact replies is an error, never clean.
    Observations are tagged.
  - **M7** (strict parsing and observations M10). INT.
- `src/test/java/com/ledgerly/receipts/internal/application/EventRegistryRecoveryTest.java`
  - Covers: scanner down, then the publication stays open, then redelivery makes the receipt ACCEPTED and closes it.
    No open publication after success. Re-delivering is harmless. The verification token never reaches
    `event_publication`.
  - **M9**. INT.
- `src/test/java/com/ledgerly/receipts/internal/application/FakeMalwareScanner.java`: helper. Not a test.

### imports
- `src/test/java/com/ledgerly/imports/internal/web/ImportApiTest.java`: **M8**. Mostly BB+S; the INT parts are noted
  inline.
  - **The 10,000-row import:**
    - 37 bad rows: 202 with Location `/api/v1/teams/{t}/imports/{id}` and **`totalRows=10000` already in the 202
      body**.
    - It ends COMPLETED with imported 9963, skipped 37, `errorsTruncated=false`, and errors at exactly the expected
      line numbers (header = line 1).
  - **Errors:**
    - Error codes `AMOUNT_INVALID` (for `abc`) and `DATE_INVALID` (for `2026-13-45`).
  - **Parsing:**
    - BOM, CRLF and quoted commas are handled. **A blank line is not a row**.
    - **Lowercase `eur` / `travel` import fine**.
    - The merchant `=cmd|...` is stored verbatim (INT DB check).
    - An unbalanced quote affects only that line.
  - **Limits:**
    - 10,001 rows give 400 and no job (INT). Exactly 10,000 rows give 202.
    - The 6th import of the day gives 429 `RATE_LIMITED` with `Retry-After` in [1, 86400].
    - The limit is per user and per team, and resets the next day (clock).
    - 8 parallel uploads give exactly 5 × 202 and 3 × 429.
    - 6 rejected (400) uploads do not use up the allowance.
  - **Bad files:**
    - A wrong header, an empty file, a header-only file or a missing column gives 400. The `note` column is optional.
    - An EXE gives 415, and **ISO-8859-1 bytes give 415**. 10 MiB + 1 gives 413.
  - **Authorization:**
    - An outsider gets 404 and no token gives 401.
    - Visible to the starter and to OWNERs; another MEMBER, an **APPROVER** and an outsider get 404. A random id gives
      404, and so does another team's path.
  - **Restart (INT):** `ImportRunner.restart` with a flaky facade; chunk size 100, so a failed 3rd chunk leaves
    `importedRows=200` and status `FAILED`, and the import completes without duplicates.
  - **Event redelivery (INT):** a redelivered `ImportQueued` event is a no-op.
- `src/test/java/com/ledgerly/imports/internal/infrastructure/ImportSkipListenerTest.java`
  - Covers the row-error recording: line, code and message. An unexpected failure becomes `ROW_INVALID` with a
    generic message (no leak). An unparseable line becomes `PARSE_ERROR`. Reader failures are not line errors. Skips
    are counted. A restart does not duplicate rows. Long messages are truncated.
  - **M8**. INT.
- `src/test/java/com/ledgerly/imports/internal/web/CapturedImportEvents.java`: helper. Not a test.

### audit
- `src/test/java/com/ledgerly/audit/internal/web/AuditApiTest.java`: **M9**. BB+S (SQL-seeded rows for paging;
  mailbox for verify; INT for checking the team-less rows).
  - **The story of one expense:**
    - create, a PATCH without If-Match (428, which leaves no event), submit, approve, reimburse.
    - The trail, newest first, is exactly `EXPENSE_REIMBURSED, EXPENSE_APPROVED, EXPENSE_SUBMITTED, EXPENSE_CREATED`,
      with `actorId` per actor, `subjectType="EXPENSE"` and `subjectId`.
  - **Edits:**
    - `EXPENSE_EDITED` has `details.changedFields == ["merchant","note"]` (in that order), and the body never contains
      the values.
  - **Teams:**
    - `TEAM_CREATED` with `subjectId` = the team.
  - **Team-less events:**
    - `USER_REGISTERED` and `USER_VERIFIED` are never in any team's trail.
  - **Who may read:**
    - OWNER 200, APPROVER 403, MEMBER 403, outsider 404, no token 401.
    - Another team's rows never show.
  - **Cursor paging:**
    - 120 rows with `limit=50` give 3 pages, no repeats, sorted by `occurredAt` desc.
    - Rows inserted mid-paging cause no repeats. The last page has **no `nextCursor`**.
    - `after=not-a-cursor`, `after=e30`, `limit=0` and `limit=101` give 400.
- `src/test/java/com/ledgerly/audit/internal/application/EveryEventReachesTheAuditTrailTest.java`
  - Covers: all 25 event types (listed in §4) reach `audit_events` with the right type and team/actor presence. The
    system has no actor for `RECEIPT_ACCEPTED` and `RECEIPT_QUARANTINED`. Events are idempotent. `details` holds ids
    and field names only (`{"changedFields": [...]}`, `{"role": "APPROVER"}`).
  - **M9**. INT.
- `src/test/java/com/ledgerly/audit/internal/infrastructure/AuditTrailTest.java`
  - Covers: the repository exposes only `append` and `page`. Raw UPDATE, DELETE and TRUNCATE fail (privileges and
    triggers). Recording is idempotent. Team-less rows are never part of a team's page.
  - **M9**. INT.
- `src/test/java/com/ledgerly/audit/AuditModuleTest.java`
  - Covers: a Modulith `Scenario`; a published `ExpenseEvent` leaves one audit row, even when it is published twice.
  - **M9**. INT.

---

## 3. Bruno collection (`bruno/`)

Prerequisites, which hold for every folder:
- The app runs with the **`local` profile and demo data** (`DevDataSeeder`, `src/main/java/com/ledgerly/shared/seed/DevDataSeeder.java`;
  `docs/deployment.md` §"Demo data").
- Environment `bruno/environments/local.bru`:
  - `host=http://localhost:8080/api/v1`
  - `teamId=00000000-0000-7000-8000-00000000a001` (team "Acme Travel")
  - `password=correct-horse-battery`
- Demo users:
  - alice: OWNER.
  - bob: APPROVER.
  - carol: MEMBER, with 5 expenses (DRAFT, SUBMITTED, APPROVED, REJECTED with a reason, REIMBURSED).
  - dave: unverified, not a member, with a pending invitation.
  - bob also has one DRAFT and alice one SUBMITTED expense, so there are 7 demo expenses in total.
- The demo seeder is **not in the plan**. It must exist (or be emulated) before any Bruno folder can run.
- The folders chain variables (`aliceToken`, `draftId`, `draftEtag`, `flowId`, `invitationId`), so they must run in
  `seq` order.

### 00-auth (login M5; `/me` M6)

| seq | Request | Asserts | Milestone |
| --- | --- | --- | --- |
| 1-4 | `login alice/bob/carol/dave` | 200, `tokenType=Bearer`, `accessToken` is a string, `expiresIn` > 0. dave is unverified but may log in. | M5 (+demo data) |
| 5 | `login wrong password` | 401 `code=UNAUTHENTICATED` | M5 |
| 6 | `login unknown email gives the same answer` | 401 `code=UNAUTHENTICATED` | M5 |
| 7 | `me without a token` | 401 `code=UNAUTHENTICATED` | M6 (`/me` is D5/M6) |
| 8 | `me with a garbage token` | `not.a.jwt` gives 401 | M6 |
| 9 | `me alice` | 200, `verified=true`, and `teams[]` has teamId = demo team with role OWNER | M6 |
| 10 | `me dave has no team` | 200, `verified=false`, `teams` empty | M6 |

### 01-expenses (behaviour M3; real-user setup M6)

| seq | Request | Asserts | Milestone |
| --- | --- | --- | --- |
| 1 | `create draft` (carol, `Idempotency-Key: {{$randomUUID}}`) | 201, DRAFT, merchant. `location` contains the id. **`etag` matches `/^"\d+"$/`**. | M3 / M6 |
| 2 | `create with too many decimals` (12.555) | 400. `code` is a string and **`traceId` is a string** | M3 / M6 |
| 3 | `create with a future date` (2999-01-01) | 400 | M3 / M6 |
| 4 | `create with a field the client may not set` (`status`) | 400 **`MALFORMED_REQUEST`** | M3 / M6 |
| 5 | `read the draft as its claimant` | 200 DRAFT | M3 / M6 |
| 6 | `read the draft as a non member` (dave) | 404 `NOT_FOUND` | M6 |
| 7 | `edit without If-Match` | 428 `PRECONDITION_REQUIRED` | M3 / M6 |
| 8 | `edit with a stale If-Match` (`"999"`) | 412 `PRECONDITION_FAILED` | M3 / M6 |
| 9 | `edit with the current If-Match` | 200, merchant Bakery. The new ETag is stored. | M3 / M6 |
| 10 | `submit without a receipt` | 422 `EXPENSE_RULE_VIOLATED` | M3 (stub) / real M7 |
| 11 | `an approver cannot edit a draft of someone else` (bob) | **403** (not 404) | M6 |
| 12 | `delete the draft` (If-Match current) | 204 | M3 / M6 |
| 13 | `the deleted draft is gone` | 404 | M3 / M6 |

### 02-reading

| seq | Request | Asserts | Milestone |
| --- | --- | --- | --- |
| 1 | `list as the owner sees the whole team` (`size=50`) | 200, `page.totalElements` ≥ 7 | M4 (+M6, demo data) |
| 2 | `list as a member sees only their own` (carol) | Every `claimantId` equals the first one | M4 / M6 |
| 3 | `filter by status` (bob, `status=SUBMITTED`) | Every item is SUBMITTED | M4 / M6 |
| 4 | `a page size over the limit is refused` (`size=100000`) | 400 | M4 / M6 |
| 5 | `sorting by an unknown field is refused` (`sort=password,asc`) | 400 | M4 / M6 |
| 6 | `export as CSV` (alice) | 200, `content-type` contains `text/csv` | M8 |
| 7 | `the audit trail is for owners only` (carol) | 403 | M9 |
| 8 | `the owner reads the audit trail` (`limit=5`) | 200 | M9 |

### 03-lifecycle (needs real ClamAV; the receipt is `bruno/files/receipt.pdf`)

| seq | Request | Asserts | Milestone |
| --- | --- | --- | --- |
| 1 | `carol creates an expense` | 201 (stores `flowId`) | M3 / M6 |
| 2 | `carol uploads a receipt` (multipart `file`) | 202, `PENDING_SCAN`, `detectedType=application/pdf` | M7 |
| 3 | `the malware scan accepts it` (pre-request sleep 4 s) | 200, `body[0].status=ACCEPTED` | M7 |
| 4 | `a member cannot approve` (carol, her own expense) | 403. It is actually still a DRAFT here, so the role check beats both the state 409 and the self-dealing 409. | M6 |
| 5 | `carol submits` | 200 SUBMITTED | M7 |
| 6 | `a submitted expense can no longer be edited` (`If-Match: "1"`) | 409 `EXPENSE_NOT_DRAFT`. This implies the version is 1 after create (0) and submit; **the receipt upload does not bump the expense version**. | M3 (+M7 setup) |
| 7 | `bob approves` | 200 APPROVED | M6 / M7 |
| 8 | `bob cannot approve twice` | 409 `EXPENSE_INVALID_TRANSITION` | M6 / M7 |
| 9 | `bob cannot reimburse` | 403 | M6 / M7 |
| 10 | `alice reimburses` | 200 REIMBURSED | M6 / M7 |

### 04-teams (M6)

| seq | Request | Asserts | Milestone |
| --- | --- | --- | --- |
| 1 | `a member cannot invite` (carol) | 403 | M6 |
| 2 | `a non member gets 404 for the team` (dave) | 404 | M6 |
| 3 | `the owner invites` (`erin@example.com`, APPROVER) | 201, `status=PENDING` | M6 |
| 4 | `the owner revokes it` | 204 | M6 |

### 05-security

| seq | Request | Asserts | Milestone |
| --- | --- | --- | --- |
| 1 | `hardening headers are on every response` (`GET /me` as alice) | 200. `x-content-type-options=nosniff`, `content-security-policy` contains `default-src 'none'`, `x-frame-options` exists. | M10 |
| 2 | `an over-long password is refused before any hashing` (login with a 200-character password) | 400 **`VALIDATION_FAILED`** | M5 |

---

## 4. Parity traps: behaviour asserted in tests (or Bruno) that `api/openapi.yaml` does not show

The spec defines statuses per operation, the `Problem` shape (`code` is a free string; only one example,
`EXPENSE_INVALID_TRANSITION`) and the schemas. Everything below goes beyond that.

### 4.1 Error catalogue (`src/main/java/com/ledgerly/shared/error/ErrorCatalogue.java`)

**Codes and types**
- The spec never enumerates the `code` values. The full set, with status and `type` = `https://ledgerly.dev/problems/<slug>`:

| code | status |
| --- | --- |
| `VALIDATION_FAILED` | 400 |
| `MALFORMED_REQUEST` | 400 |
| `UNAUTHENTICATED` | 401 |
| `FORBIDDEN` | 403 |
| `NOT_FOUND` | 404 |
| `EXPENSE_INVALID_TRANSITION` | 409 |
| `EXPENSE_SELF_DEALING` | 409 |
| `LAST_OWNER` | 409 |
| `ALREADY_MEMBER` | 409 |
| `INVITATION_NOT_PENDING` | 409 |
| `INVITATION_EXPIRED` | 409 |
| `EMAIL_NOT_VERIFIED` | 403 |
| `EXPENSE_NOT_DRAFT` | 409 |
| `IDEMPOTENCY_IN_PROGRESS` | 409 |
| `PRECONDITION_FAILED` | 412 |
| `PRECONDITION_REQUIRED` | 428 |
| `EXPENSE_RULE_VIOLATED` | 422 |
| `IDEMPOTENCY_KEY_REUSED` | 422 |
| `PAYLOAD_TOO_LARGE` | 413 |
| `UNSUPPORTED_MEDIA_TYPE` | 415 |
| `RECEIPT_DUPLICATE` | 409 |
| `RATE_LIMITED` | 429 |
| `BATCH_REJECTED` | 422 |
| `INTERNAL_ERROR` | 500 |
| `REQUEST_REJECTED` | 400 by default |

- Titles are fixed per code. `AuthApiTest.unknownEmailAndWrongPasswordLookTheSame` compares `title`.
- **An unknown route gives 404 with `code=REQUEST_REJECTED`**, not `NOT_FOUND` (`ExpensesApiTest.unknownPathIsAProblemToo`).

**When the problem body appears**
- `traceId` is on **every** problem, including 400s (Bruno `01-expenses/create with too many decimals.bru`). The spec
  mentions `traceId` only for 500.
- Every error is `application/problem+json`: 401s (`AuthApiTest.unauthenticatedResponsesAreProblemDetails`), 404s and
  405s (`Api8...notFoundAndBadMethodsAreProblemsNotWhitelabelPages`), and 500s.
- The 500 body keys are ⊆ {type, title, status, code, traceId, instance, detail}, with no exception text.

**Codes that depend on the kind of input error**
- **Unknown JSON property gives 400 `MALFORMED_REQUEST`**: Jackson runs with `fail-on-unknown-properties: true`
  (`src/main/resources/application.yaml`). Bean-validation failures give 400 **`VALIDATION_FAILED`** with `errors[]`.
  Bruno asserts both codes.
- **`errors[].field` names:**
  - Too many decimals or a bad currency: `money` (not `money.amount`).
  - Future date: `incurredOn`.
  - Sort by amount without a currency: `sort`.
  - Atomic batch: `items[<i>].<field>`.
- The login password limit is 128 characters. A longer one gives 400 `VALIDATION_FAILED` (Bruno 05).
- `401` responses carry **`WWW-Authenticate: Bearer...`** (`AuthApiTest.unauthenticatedResponsesAreProblemDetails`).
- The import row-error codes are not in the spec:
  - `AMOUNT_INVALID` and `DATE_INVALID` (`ImportApiTest.errorsNameTheKindOfProblemAndTheLine`).
  - `ROW_INVALID` and `PARSE_ERROR` (`ImportSkipListenerTest`).

### 4.2 Status precedence and ordering (not expressible in the spec)

**Body size before authentication**
- A body over the limit gives 413 before authentication: a 2 MiB JSON body with **no token** gives 413
  (`BatchAndExportApiTest.theBodyLimitAppliesBeforeAuthenticationToo`).
- This also holds for chunked bodies (`ChunkedBodyLimitTest`).
- The spec lists no 413 on createExpense, login and others.

**Authentication before routing**
- Without a token, any non-public path gives **401**, even an unknown one (actuator and Swagger on the public port:
  401 without a token, 404 with one; `Api8...actuatorIsNotReachableOnThePublicPort`).

**Visibility, then role, then state, then rule**

| Caller and action | Result | Test |
| --- | --- | --- |
| Not a teammate, any action | 404, not 403 | `RoleMatrixTest` |
| MEMBER on a colleague's expense | 404 | `RoleMatrixTest` |
| APPROVER or OWNER: PATCH, DELETE, `:submit`, `:withdraw` or `:reopen` on someone else's expense | **403** | `RoleMatrixTest` |
| APPROVER or OWNER: `:submit` on someone else's DRAFT | **403**, not the 422 receipt rule | `RoleMatrixTest` |
| MEMBER `:approve` on their own SUBMITTED expense | 403, not the 409 self-dealing | `Api5.aMemberCallingApproveGets403` |
| carol `:approve` on her own **DRAFT** | 403, not the 409 invalid transition | Bruno `03-lifecycle` seq 4 |
| APPROVER `:approve` on their own SUBMITTED expense | 409 `EXPENSE_SELF_DEALING` | — |
| OWNER `:reimburse` on their own APPROVED expense | 409 `EXPENSE_SELF_DEALING` | — |
| APPROVER `:reimburse` | 403 | — |

**Receipts**
- An APPROVER uploading to someone else's draft gets **403**. A MEMBER colleague gets **404**
  (`ReceiptsApiTest.onlyTheClaimantMayUpload`).
- Deleting a receipt of a SUBMITTED expense: the claimant gets 409 and the approver gets 403.

**Imports**
- A non-starter who is not an OWNER gets 404, even as APPROVER. That is not 403.

**Invitations**
- Accepting someone else's invitation gives **404**, not 403.

**If-Match**
- The value is a quoted integer version (`"0"`, `"1"`, ...).
- A non-numeric `If-Match` (`garbage`) gives **412**, not 400.
- A missing `If-Match` on PATCH or DELETE gives 428.
- Every transition bumps the version. A receipt upload does not (Bruno 03, seq 6, asserts `"1"` after submit).

### 4.3 Headers not in the spec

**Security headers** (M10). They are on every response, including 401 and 404 (`Api8...everyResponseCarriesTheSecurityHeaders`):
- `Strict-Transport-Security: max-age=31536000; includeSubDomains`. It is sent even over plain http (LL:126).
- `Content-Security-Policy: default-src 'none'; frame-ancestors 'none'`. This is an **exact** string; a looser one
  is used only for Swagger in local.
- `X-Content-Type-Options: nosniff`
- `X-Frame-Options: DENY`
- `Referrer-Policy: no-referrer`
- `Cache-Control` containing `no-store`.

**Cache-Control on login and download**
- On login (M5) the value is **exactly `no-store`**, an equality check (`AuthApiTest.loginReturnsAccessTokenAndHardenedRefreshCookie`).
- On the receipt download link it is `no-store` (`ReceiptsApiTest`).
- So the global M10 value must stay exactly `no-store`. Spring's default is
  `no-cache, no-store, max-age=0, must-revalidate`, which would fail the equality check.

**CORS** (`Api8`; `SecurityConfig.java:127` sets max-age 3600)
- The allowlist holds exact origins.
- A preflight from an allowed origin gives 200 with:
  - `Access-Control-Allow-Origin` equal to the origin (never `*`).
  - `Access-Control-Allow-Credentials: true`.
  - `Access-Control-Allow-Headers` including `authorization`.
- Actual responses carry `Access-Control-Expose-Headers` including `ETag` and `Retry-After`.
- A **disallowed origin gives 403 on the preflight AND on the actual authenticated GET**, with no ACAO.
- Bad allowlist entries prevent start-up.

**Set-Cookie** (the spec only describes it in prose)
- Login and refresh: `refresh_token=<v>; Path=/api/v1/auth; HttpOnly; Secure; SameSite=Strict` plus `Max-Age` (TTL
  14 days).
- Logout: `Max-Age=0`.
- There is **no Set-Cookie on a 401** login.

**Location**
- Create expense: `/api/v1/teams/{t}/expenses/{id}`.
- Create team: `/api/v1/teams/{id}`.
- Import: `/api/v1/teams/{t}/imports/{id}`.
- **Receipt upload: the receipts list `/api/v1/teams/{t}/expenses/{e}/receipts`**, not the receipt itself.

**CSV export**
- `Content-Disposition: attachment; filename=...expenses.csv`.
- `X-Truncated` is a lower-case string `true`/`false`.

**Rate limiting**
- `Retry-After` and code `RATE_LIMITED` appear on **every** rate-limited request: per user on all authenticated
  routes, and per client IP on `/api/v1/auth/**`. The spec documents 429 only on `POST imports`.
- `Retry-After` is in seconds, [1,60] for request limits and [1,86400] (until UTC midnight) for imports.

### 4.4 Limits, values and formats that the spec under-specifies or contradicts

**Paging and lists**
- `size>100` gives 400 (the spec has `maximum`). `size=0`, `page=-1`, `size=abc`, `limit=0` and `limit=101` give 400.
  A page past the end gives 200 with an empty list.
- **`currency=ZZZ` gives 400** even though it matches the spec pattern `^[A-Z]{3}$`. It must be a real ISO currency;
  the pseudo codes XAU, XTS and XXX are rejected (`MoneyTest`).
- `incurredFrom > incurredTo` gives 400 (list and export). The range is inclusive.
- Sort values that the spec pattern would also reject are tested to give 400, and the default sort tie-breaker is
  `id` (pages never overlap).
- A MEMBER's list and export are silently forced to `claimant=self`, and `totalElements` counts only their own rows.

**Money and text fields**
- The scale is per currency (EUR ≤ 2 decimals, JPY 0, KWD 3), although the spec allows up to 4 decimals. Zero and
  negative amounts are rejected.
- Responses normalise the amount (`"12.50"`, `"10.00"`).
- **A whitespace-only `merchant` (`"   "`) is invalid** (batch `problem.field=merchant`), although the spec only says
  `minLength: 1`.
- `incurredOn` must not be in the future, relative to the server clock (UTC). Today is allowed.
- The JSON body limit is 1 MiB, and the response is 413 `PAYLOAD_TOO_LARGE`.

**Batch**
- A `BatchExpenseItem` with an unknown property (`status`, `claimantId`) gives a **400 for the whole request**. The
  spec does *not* set `additionalProperties: false` on `BatchExpenseItem`.
- An empty `items` gives 400.
- Rejected items carry no `id`.

**Receipts**
- Detection works on magic bytes; `text/plain` and HTML give 415, and an empty file gives 400.
- Exactly 10 MiB is accepted and 10 MiB + 1 gives 413. A missing `file` part gives 400.
- `originalName` keeps traversal strings verbatim but **strips control characters**.
- The download `expiresAt` is ≤ 5 min. The presigned link forces an attachment download under the sanitised name
  (`S3ObjectStorageTest.aPresignedLinkDownloadsAsAnAttachmentUnderTheGivenName`, `X-Amz-Expires=300`). How this is
  encoded in the URL was not checked.

**Import**
- The header must be exactly `date,merchant,amount,currency,category[,note]`.
- A UTF-8 BOM and CRLF are accepted. **Blank lines are not rows** and do not count in `totalRows`.
- **Currency and category are case-insensitive in an import** (`eur`, `travel` are accepted). The JSON API rejects a
  lowercase currency.
- Non-UTF-8 gives 415, and binary gives 415.
- An empty file, a header-only file, a wrong header, a missing column or more than 10,000 rows gives 400.
- `totalRows` is already present in the 202 response.
- Errors are reported by physical line number.
- The daily cap is **per user and per team**, counts only accepted uploads, and is safe under concurrency (8 parallel
  uploads give exactly 5 accepted).
- Visibility: the starter and OWNERs only.

**Export CSV**
- The header row and column order are `id,incurredOn,merchant,amount,currency,category,status,claimantId,note`.
- Lines end in CRLF and follow RFC 4180 quoting.
- A cell that starts with `= + - @ \t \r` gets a `'` prefix, applied before quoting.
- The cap is 10,000 rows (configurable).

**Auth**
- Register passwords are 12-128 characters.
- The register body is identical for new, taken and case-variant emails. Emails are lower-cased.
- Verification:
  - The token is single-use and expires after 24 h.
  - Reuse, expiry or a guess gives 400. That is not 401 or 404.
- Access tokens:
  - Their lifetime is 600 s, matching `expiresIn`.
  - The JWT validator allows 30 s of clock skew (`IdentityConfig.java:68`).
  - JWTs are ES256 with a `kid`, and the claims are exactly sub, iat, exp and jti, with `sub` = the Ledgerly user id.
  - JWKS `keys[]` hold an EC P-256 key without `d`.
- Refresh and logout:
  - The `X-Requested-With` value must be exactly `ledgerly`; anything else gives 400, and the refresh token is not
    consumed.
  - Refresh without a cookie gives 401. Logout without a cookie gives 204.
  - A refresh-token replay revokes the whole family.
- Lockout:
  - 5 failures lock the address for 15 min, even against the right password.
  - The failures are counted **per email whether or not the account exists** (LL:125).
  - A success resets the counter.
  - The locked 401 is indistinguishable: its detail never mentions "lock".
- Rate limits:
  - The defaults are 600 requests/min per user and 60/min per IP on `/api/v1/auth/**` (`LedgerlyLimits.java:20-21`).
  - The user limit uses a gradual refill (Api4 expects *exactly* 10 of 40 refused at 30/min).

**Timing-equal login**
- An unknown email still runs an Argon2 `matches` against a dummy hash (`src/main/java/com/ledgerly/identity/internal/application/Login.java:51,70`).
- Registering a taken email still hashes the password (LL:45).
- No test measures timing; only response equality is asserted. It is still a parity requirement for the
  enumeration-resistance claim.

**Teams**
- Invitations expire after 7 days. The status is computed on read, with no scheduler.
- Re-inviting the same email (case-insensitive) revokes the old pending invitation, whose id then gives 409.
- Inviting an existing teammate gives 201 and looks identical.
- The invitation response has the **same property set** for registered and unregistered emails.
- `/me` has **exactly** five top-level keys. Invitations are listed only for a verified email and only while
  unexpired and pending.
- Roles are read from the DB on every request, so a demotion is immediate for the same token.
- A removed claimant gets 404 on their own expense, but approvers can still decide it, and it keeps its claimant.

**Expense responses**
- They must not contain `version`, `createdAt` or `updatedAt`, and the field set is ⊆ the 10 documented keys.
- `rejectionReasons` keeps every rejection, in order.

**Idempotency**
- The key is scoped per user and lives 24 h (`IdempotencyStore.java:21`).
- A replay returns 201 with the same id and the *current* state (LL:23).
- A different body gives 422 `IDEMPOTENCY_KEY_REUSED`.
- `IDEMPOTENCY_IN_PROGRESS` 409 exists in the catalogue.

**IDs**
- Expense ids are UUIDv7 (`UuidV7Test.expenseIdsUseIt`). The spec only says `format: uuid`.

### 4.5 Audit trail content (the spec has `eventType: string` and `details: object`)

**Event types and actors**
- The 25 `eventType` values (`EveryEventReachesTheAuditTrailTest.java:60-96`):
  - Team-less: `USER_REGISTERED`, `USER_VERIFIED`, `REFRESH_TOKEN_REUSE_DETECTED`.
  - Teams: `TEAM_CREATED`, `MEMBER_INVITED`, `INVITATION_REVOKED`, `INVITATION_ACCEPTED`, `INVITATION_DECLINED`,
    `MEMBER_ROLE_CHANGED`, `MEMBER_REMOVED`, `MEMBER_LEFT`.
  - Expenses: `EXPENSE_CREATED`, `EXPENSE_EDITED`, `EXPENSE_DELETED`, `EXPENSE_SUBMITTED`, `EXPENSE_WITHDRAWN`,
    `EXPENSE_APPROVED`, `EXPENSE_REJECTED`, `EXPENSE_REOPENED`, `EXPENSE_REIMBURSED`.
  - Receipts: `RECEIPT_UPLOADED`, `RECEIPT_ACCEPTED`, `RECEIPT_QUARANTINED`, `RECEIPT_DELETED`.
  - Imports: `IMPORT_COMPLETED`.
- `RECEIPT_ACCEPTED` and `RECEIPT_QUARANTINED` have **no `actorId`**.

**Details and subjects**
- `EXPENSE_EDITED`: `details.changedFields` lists the field names in the order `["merchant","note"]`, never the
  values.
- `MEMBER_INVITED`: `details = {"role": ...}`.
- Expense subjects: `subjectType="EXPENSE"`, `subjectId` = the expense id.
- `TEAM_CREATED`: `subjectId` = the team id.

**What gets recorded**
- Refused requests (e.g. a 428) leave no event.
- Team-less events never appear in any team's trail.

**Ordering and cursor**
- The order is newest first by **write time `created_at`**, then id. `occurredAt` is what the client sees.
- `nextCursor` is **absent** on the last page.
- The cursor is opaque Base64url JSON. Junk or `e30` (`{}`) gives 400.
- The default `limit` is 50.

**Timing**
- The trail is eventually consistent (asynchronous listeners), so tests poll for up to 30 s.

### 4.6 Operational surface the spec does not describe at all

**Management port**
- It is 8081 by default (`application.yaml`).
- It serves:
  - `/actuator/health` (`{"status":"UP"}`).
  - `/actuator/health/liveness` and `/readiness`.
  - `/actuator/info`.
  - `/actuator/prometheus`.
- Everything else on the management port gives ≠200.

**Prometheus**
- The metric names follow the Micrometer conventions:
  - `ledgerly_expenses_transitions_total{action="submit"...}`
  - `ledgerly_receipts_quarantined_total`
  - `ledgerly_imports_rows_skipped_total`
- **`jvm_memory_used_bytes` and `http_server_requests`** are JVM- and Spring-specific names that the test asserts.
  Decided after M1: this part is Java-only. The Rust port asserts its equivalents instead,
  `process_resident_memory_bytes` and `http_server_request_duration_seconds` (OpenTelemetry's
  `http.server.request.duration`). The `ledgerly_*` names stay Java's.
- Labels hold no emails and no ids.
- Counters count successful transitions only.

**Swagger and docs**
- In the `local` profile:
  - `/docs` redirects to `/swagger-ui/index.html` (no token needed).
  - `/swagger-ui/openapi.yaml` is **byte-identical** to `api/openapi.yaml`.
  - `/swagger-ui/swagger-ui-bundle.js` is served.
  - `POST /docs` gives 401.
- In any other profile they give 401 without a token and 404 with one.

**Keycloak profile** (M11)
- `/api/v1/auth/{login,register,refresh}` and the JWKS give 404.
- Tokens for another audience give 401.
- An unreachable IdP gives 401 `UNAUTHENTICATED` (problem+json, not 500).

**Logs**
- Logs are ECS JSON with a `message` field.
- No emails, passwords, tokens, cookies, `Bearer `, receipt names or content, merchants, notes, `password_hash` or
  `argon2id` may appear (`LogHygieneTest`, `AuthApiTest.logsNeverContainPasswordsOrTokens`).
- The team id **must** appear in the request log lines (MDC).
- The local profile logs the verification token (`LocalVerificationLogger`). It is not a secret leak there; it is the
  only built-in "mailbox".

**Start-up**
- The `prod` profile refuses to start without the DB, keys, S3 and ClamAV settings, and lists every missing setting
  (`ProductionEnvironmentCheckTest`).
- An invalid CORS allowlist also refuses start-up.

**Demo data**
- `local` demo data (fixed team id, four users, one password, 7 expenses, dave's invitation) is required by every
  Bruno folder and is not in the spec or the plan.

**Asynchronous behaviour that tests poll for**
- Receipt scan (≤15-20 s).
- Import completion (≤30-90 s).
- Audit rows (≤30 s).
- Receipt clean-up after an expense delete (≤10 s).

### 4.7 Test hooks the Rust SUT will need for full parity (derived from the above)
1. **Clock control**, for lockout, token expiry, verification and invitation expiry, refresh TTL, the daily import
   reset and the rate-limit refill.
2. **Mailbox**: a way to get the verification token. Without it no new user can accept an invitation, so multi-role
   teams cannot be built over HTTP.
3. **Scanner control**: EICAR detection (real ClamAV is fine) and an outage switch.
4. **Config at start-up**: rate limits, allowed origins, `max-export-rows`, the management port.
5. **A fault route** for the 500 test (`/api/v1/test-only/boom`), and an exclusion of it from the inventory check.
6. **Profiles**: `local` (Swagger, demo data, the verification log) vs non-local, plus `keycloak`.
