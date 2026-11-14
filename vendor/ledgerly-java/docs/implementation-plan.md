# Ledgerly implementation plan

[`CONCEPT.md`](../CONCEPT.md) says **what** Ledgerly is and which milestones teach what. This plan says **in which order to build it**: for every milestone, the concrete steps, the tests to write first, the abuse tests and the files that appear. Vocabulary follows [`GLOSSARY.md`](../GLOSSARY.md); where anything here disagrees with the glossary, the glossary wins.

You write the code. Each step is small enough for one evening and ends with a green `./mvnw verify`.

---

## 1. Decisions settled before milestone 0

Writing the plan turned up gaps in the concept: a few module cycles and features that are needed before the milestone that builds them. All eight were settled on 2026-11-05 and are reflected in `CONCEPT.md`; D3 and D4 are recorded as [ADR 0002](adr/0002-expenses-asks-receipts-through-an-interface-it-owns.md) and [ADR 0003](adr/0003-commands-through-facades-not-only-events.md).

| # | Gap | Decision |
| --- | --- | --- |
| D1 | `CONCEPT.md` shows the project under `ledgerly/`, but the repository is `teach-me-java-backend`. | Put the Maven project at the repository root, next to `CONCEPT.md`. Use the package `com.ledgerly`. |
| D2 | Expenses get endpoints in M3, but logins only arrive in M5 and teams in M6. Who is the caller before then? | Add an interface `CurrentUser` to `shared` and a temporary adapter that reads `X-Debug-User-Id` and `X-Debug-Role` headers. The adapter is active **only** in the `local` and `test` profiles. A test asserts the `prod` profile has no such bean. Delete the adapter in M5 (user) and M6 (role). |
| D3 | Receipts must check that their expense is a Draft (receipts → expenses). Submitting must check that receipts are Accepted (expenses → receipts). That is a cycle, which Modulith forbids. *ADR 0002* | Expenses declares a public interface `ReceiptChecks` (`summaryFor(expenseId)` → counts by scan status) and receipts implements it. Both arrows now point from receipts to expenses. This is dependency inversion between modules. |
| D4 | Rule 2 says "writes across modules only through events", but an Import has to *create* expenses. *ADR 0003* | Allow commands through the owning module's facade: imports calls `ExpensesFacade.createDrafts(...)`. Change rule 2 to say: *"Never touch another module's tables or repositories; write through its facade (commands) or react to its events."* |
| D5 | `GET /me` needs users (identity) and roles and invitations (teams). If identity serves it, identity ↔ teams becomes a cycle. | Serve `/me` from the teams module. Teams already reads identity's facade, for example to check Verified. |
| D6 | Spring Modulith is first used in M9, but boundary violations creep in from M1. | Add the `ApplicationModules.verify()` test in **M1**. Keep the event publication registry and `@ApplicationModuleListener` for M9: building with plain `@TransactionalEventListener` first lets you *see* the lost-event problem the registry solves. |
| D7 | The OWASP API10 row tests CSV formula injection "on export", but there is no export endpoint. | Add `GET /teams/{teamId}/expenses.csv`, the same filters as the list without paging and capped at 10,000 rows, in M8. The alternative is to drop "export" from the API10 row and instead test that imported formula cells are stored as text. |
| D8 | M4 asks for keyset paging, but the audit trail, the only cursor-paged resource, arrives in M9. | In M4, build the cursor codec and the keyset query helper in `shared` and test them against the expenses table. M9 wires them to `/audit-events`. |

---

## 2. Module dependencies (allowed arrows)

```text
            shared  ◄──────────── everyone
               ▲
identity ◄── teams ◄── expenses ◄── receipts
                           ▲
                           └────── imports
audit ──► events packages of identity, teams, expenses, receipts, imports
```

- An arrow means "may call the facade of" or "may listen to the events of".
- Imports also calls the teams facade (to know the caller's role: OWNERs may see every import of the team). That is one more arrow pointing downward, so still no cycle.
- Nothing points back up, so there are no cycles. The Modulith test from D6 enforces this.
- Each module's `events` package is a public named interface (`@NamedInterface`). Audit depends on events only, never on facades.

---

## 3. Schema roadmap (Flyway)

| Migration | Milestone | Tables |
| --- | --- | --- |
| `V1__expenses.sql` | 2 | `expenses`, `expense_rejections` (one row per rejection, so every Rejection reason is kept) |
| `V2__idempotency_keys.sql` | 3 | `idempotency_keys` |
| `V3__identity.sql` | 5 | `users`, `email_verifications`, `refresh_tokens` |
| `V4__teams.sql` | 6 | `teams`, `memberships`, `invitations` |
| `V5__receipts.sql` | 7 | `receipts` |
| `V6__imports.sql` | 8 | Spring Batch metadata schema (copied from `spring-batch-core`, not `initialize-schema`), `import_jobs`, `import_row_errors` |
| `V7__event_publication.sql` | 9 | Modulith `event_publication` |
| `V8__audit.sql` | 9 | `audit_events`, plus `REVOKE UPDATE, DELETE` for the app's DB role |

Rules for every migration:
- Never edit a migration after it has been merged; add a new one instead.
- Hibernate runs with `ddl-auto=validate`.
- No foreign keys across modules. `expenses.team_id` is a plain `uuid`, because modules own their tables and could be split apart later.
- Every table that holds team data has an index that starts with `team_id`.

---

## 4. The per-endpoint recipe (from M3 onward)

Every new endpoint goes through the same nine steps, so they're written down once here:

1. Add the path, schemas, examples and every error response to `api/openapi.yaml`, then run the linter.
2. Run `./mvnw generate-sources` and look at the generated interface.
3. Write a failing `RestTestClient` test for the happy path.
4. Write the use case in `internal/application` and the domain behaviour in `internal/domain`.
5. Implement the generated interface in `internal/web` and map DTOs ↔ domain (no entity ever leaves the module).
6. Write the abuse tests: wrong team (404), wrong role (403), invalid input (400 with `errors`), wrong state (409), stale `If-Match` (412).
7. Add a request to `http/<module>.http`.
8. Publish the domain event if the endpoint changes state (audit picks it up in M9).
9. Add a line to the learning log.

---

## 5. Milestones

Each milestone gets its own branch (`m<N>-<slug>`) and pull request, and ends with the tag `m<N>` once the definition of done in `CONCEPT.md` §9 is met.

### M0 · Plain-Java core (no Spring)

**Goal:** Money and the Expense state machine, correct and fully unit-tested, as plain Java.

Setup
- [ ] `pom.xml` with Java 25, JUnit Jupiter and AssertJ only. Add the Maven wrapper (`mvn wrapper:wrapper`), `.sdkmanrc` or `mise.toml`, and `.editorconfig`.
- [ ] Packages: `com.ledgerly.shared.money`, `com.ledgerly.shared.id`, `com.ledgerly.expenses.internal.domain`. This code carries straight into the Boot app in M1.

Money (`shared.money`), tests first
- [ ] `Money(BigDecimal amount, Currency currency)` as a record whose compact constructor validates.
- [ ] Reject null, zero and negative amounts.
- [ ] Reject a scale above the currency's minor units: EUR `10.001` ✗, JPY `1.5` ✗, KWD `1.001` ✓.
- [ ] Normalise the scale so that `EUR 10.0` equals `EUR 10.00` (test `equals` and `hashCode` together).
- [ ] `plus` across currencies throws, and so does `compareTo` across currencies.
- [ ] Decide which "active ISO 4217" codes to accept. `java.util.Currency` also knows special codes (XAU, XTS) that report `-1` fraction digits; reject those. Write the rule down in the learning log.

Typed ids (`shared.id`)
- [ ] `UserId`, `TeamId`, `ExpenseId`, … as records that wrap `UUID`. Adding a UUIDv7 generator in M2 is enough.

Expense (`expenses.internal.domain`), tests first
- [ ] `ExpenseStatus` enum and `Category` enum.
- [ ] Factory `Expense.draft(teamId, claimantId, money, category, incurredOn, merchant, note, Clock)`:
  - `incurredOn` can't be in the future (inject `Clock`).
  - merchant is required, and merchant and note are length-capped.
- [ ] `Actor(UserId id, Role role)`. The domain receives the role; it never looks it up.
- [ ] The six transitions from the `CONCEPT.md` table:
  - One **parameterised test over all 5 statuses × 6 actions**: legal pairs succeed, the other 24 throw `InvalidTransition`.
  - Role tests for each action.
  - Self-dealing tests: approve, reject or reimburse your own expense → `SelfDealing`.
- [ ] `submit(actor, ReceiptSummary)` where `ReceiptSummary(int accepted, int pendingScan)`. It needs ≥ 1 accepted and 0 pending. This is where D3 starts.
- [ ] `reject` needs a non-blank, capped reason. Each rejection is appended to a list and nothing is overwritten.
- [ ] `edit(...)`, `canAddOrRemoveReceipts()` and `canDelete()` are only allowed in DRAFT and only for the claimant.
- [ ] `isVisibleTo(actor)`: MEMBERs see only their own expenses.

Done when: `./mvnw test` is green and you can explain why `Money` is a record and `Expense` is not.

### M1 · First Boot app

- [ ] Switch the parent to `spring-boot-starter-parent` 4.1.x (pin the patch version). Add the web and actuator starters and `LedgerlyApplication`. The M0 tests must stay green.
- [ ] Add `application.yaml` and `application-local.yaml`:
  - ECS JSON console logs everywhere except `local`.
  - Actuator on management port 8081, exposing `health` and `info` only.
- [ ] Add a `@ConfigurationProperties` record `LedgerlyLimits` with page 100, batch 100, upload 10 MB, import rows 10,000 and imports per day 5. Bind it with `@Validated`. Every later limit reads from here.
- [ ] Create `compose.yaml` with `grafana/otel-lgtm`, add `spring-boot-docker-compose`, and wire up OTLP export of traces and logs.
- [ ] **D6:** add a `ModularityTest` that calls `ApplicationModules.of(LedgerlyApplication.class).verify()`.
- [ ] Code quality:
  - Spotless, plus NullAway with JSpecify on Error Prone. Add `@NullMarked` in `package-info.java` per module.
  - `.pre-commit-config.yaml` with gitleaks and Spotless.
- [ ] `.github/workflows/ci.yml` running `./mvnw verify` and gitleaks.
- [ ] `.env.example` and `docs/learning-log.md`.

Tests
- Context loads.
- `GET :8081/actuator/health` returns UP.
- A log line captured with `OutputCaptureExtension` parses as JSON.

### M2 · Persist expenses

- [ ] Add the JPA, Flyway and PostgreSQL dependencies and a `postgres` service in compose. For tests, add `spring-boot-testcontainers` and a `@ServiceConnection` Postgres in a shared `TestcontainersConfiguration`.
- [ ] `V1__expenses.sql`:
  - `amount numeric(19,4)`: 4 decimal places covers every currency (CLF has 4); the domain enforces the exact scale.
  - `currency char(3)`, `version bigint`, `created_at`, `updated_at`.
  - Indexes `(team_id, incurred_on desc, id)`, `(team_id, claimant_id)` and `(team_id, status)`.
- [ ] Map `Expense` as an entity:
  - `Money` as an `@Embeddable` record.
  - Protected no-arg constructor, no setters, `@Version`.
  - Rejections as an `@ElementCollection` (or an entity) in `expense_rejections`.
- [ ] Write a UUIDv7 generator in `shared.id` and test that ids are time-ordered. Ids are created in the domain, so they are known before `persist`.
- [ ] Have `ExpenseRepository` extend **`Repository`, not `JpaRepository`**, and declare only `findByTeamIdAndId`, `save` and `delete`. With no inherited `findById`, a query that isn't team-scoped can't even be written. This is the first OWASP API1 control.
- [ ] Add `CreateExpense` and `GetExpense` use cases with `@Transactional` (no web layer yet).

Tests
- Repository round trip against real Postgres.
- Flyway and Hibernate validate together.
- Two copies of the same expense are saved one after the other → the second save throws an optimistic-locking failure.
- An expense of team A is not found with team B's id.

### M3 · Contract, validation, errors

- [ ] **D2:** add `CurrentUser` and the debug-header adapter (`local` and `test` only), plus a test that the `prod` profile has no such bean.
- [ ] Add a `MembershipLookup` port in `expenses.internal.application` with a stub adapter that reads the debug role (replaced in M6).
- [ ] Add the **D3** `ReceiptChecks` interface in `expenses` with a test double; the real implementation comes in M7.
- [ ] `api/openapi.yaml`, version 3.1, contains:
  - All expense endpoints except batch, list filters and export.
  - Shared schemas: `Money` with **`amount` as a decimal string** (`"12.50"`, so JSON never touches floats), `ProblemDetail` and its extensions, and an `Error` catalogue as reusable responses.
  - The `bearerAuth` security scheme, declared now.
- [ ] Linting: run Redocly CLI or Spectral in CI and pre-commit.
- [ ] `openapi-generator-maven-plugin` with the `spring` generator, `interfaceOnly=true`, Bean Validation and Jakarta settings.
- [ ] Errors:
  - A sealed `LedgerlyException` hierarchy: `NotFound`, `Forbidden`, `Conflict` (including `InvalidTransition` and `SelfDealing`), `RuleViolated`.
  - An `ErrorCatalogue` enum (code, type URI, title, status).
  - One `@RestControllerAdvice` that also handles bean validation errors (→ `errors`), unreadable bodies (400) and unknown exceptions (500 with only the `traceId`).
- [ ] `ETag: "<version>"` on GET. PATCH and DELETE **require** `If-Match` (428 if it's missing, 412 if it's stale).
- [ ] `Idempotency-Key` on create:
  - `V2__idempotency_keys` with key, user, request hash, stored status and body, and an expiry.
  - Same key and same body → replay the stored response. Same key and a different body → 422.
  - *This is the hardest piece of M3; it's fine to do it last.*
- [ ] Transition endpoints: `:submit`, `:withdraw`, `:approve`, `:reject`, `:reopen`, `:reimburse`.
- [ ] Contract drift test (API9): every `RequestMappingHandlerMapping` path is in the spec, and every spec path is mapped. Serve the static file to Swagger UI in `local` only.
- [ ] Add `http/expenses.http`.

Abuse tests
- PATCH with `status` or `claimantId` in the body is rejected (API3).
- A MEMBER calling `:approve` gets 403.
- Approving your own expense gets 409.
- `amount: 12.345` in EUR gets 400.
- A future `incurredOn` gets 400.

### M4 · Paging and filtering

- [ ] `GET /teams/{teamId}/expenses`:
  - Query parameters: `page` and `size`, where `size > 100` → **400**. Spring would silently clamp; reject explicitly and document it.
  - `sort` from an allowlist of `incurredOn`, `createdAt` and `amount`. Any other field → 400. `amount` without a `currency` filter → 400.
  - Filters for status, category, claimant, currency, `incurredFrom` and `incurredTo`, built as JPA `Specification`s.
- [ ] A MEMBER's list is always forced to `claimant = self`, whatever filter they send.
- [ ] Return the `PagedModel` shape (`@EnableSpringDataWebSupport(pageSerializationMode = VIA_DTO)`) and mirror it in the spec.
- [ ] **D8:**
  - `shared.paging.Cursor`: a record of `(Instant, UUID)` encoded as Base64url JSON. Decoding junk → 400.
  - A `JdbcClient` keyset helper: `WHERE team_id = ? AND (created_at, id) < (?, ?) ORDER BY created_at DESC, id DESC LIMIT ?+1`, where the extra row tells you whether there is a next page.
  - Test it against `expenses`.

Tests
- Paging through 250 seeded expenses visits every one exactly once.
- Inserting rows while paging with a cursor doesn't produce duplicates.
- Sort injection (`sort=password`) gets 400.

### M5 · Authentication (identity module)

- [ ] `V3__identity`:
  - `users` with the email stored normalised (lower-cased), `verified` and `locked_until`.
  - `email_verifications` holding only the token **hash** and its expiry.
  - `refresh_tokens` with `family_id`, `token_hash`, `expires_at`, `revoked_at` and `replaced_by`.
- [ ] Argon2id via `Argon2PasswordEncoder`, which needs Bouncy Castle.
- [ ] `register`:
  - Always 202 with the same body, whether the email is new or taken, so emails can't be enumerated.
  - The verification link is logged as an event, without the token, in `local` only.
- [ ] `verify` uses a single-use token from 32 `SecureRandom` bytes that expires after 24 h.
- [ ] `login` gives the same 401 for an unknown email and a wrong password, and locks the account after N failures for M minutes (a simple counter; Bucket4j comes in M10). Unverified users *may* log in; Verified only gates accepting invitations.
- [ ] JWT:
  - ES256 key pair, generated at startup in `local` and `test`, loaded from env in `prod`.
  - `NimbusJwtEncoder`, with a `kid`.
  - `/.well-known/jwks.json` is permitted and documented.
  - Claims are `sub` = user id, `iat`, `exp` (10 min) and `jti` only. **No email, no roles**: roles are per team and read from the DB on every call, so demoting someone takes effect immediately.
- [ ] Resource server:
  - `oauth2ResourceServer().jwt()` with a decoder built from the local public key (no HTTP call to itself).
  - Deny by default; permit `/auth/**`, the JWKS endpoint, and Swagger in `local`.
- [ ] Refresh:
  - The cookie is `HttpOnly; Secure; SameSite=Strict; Path=/api/v1/auth`.
  - Every refresh rotates the token. Presenting an already-rotated token **revokes the whole family**.
  - Write a short ADR on CSRF: why `SameSite=Strict` plus a required custom header is enough here.
- [ ] `logout` revokes the family and clears the cookie.
- [ ] Swap the `CurrentUser` adapter to read the `Jwt` and **delete the debug user header**. A servlet filter puts `userId` in the MDC.
- [ ] Inject a `Clock` everywhere; tests use a mutable test clock.

Abuse tests
- Expired, tampered, `alg: none` and wrong-key tokens all get 401.
- A replayed refresh token revokes the family.
- A locked account is rejected even with the right password.
- Captured logs contain no password, token or cookie value.

### M6 · Teams and authorization

- [ ] `V4__teams`:
  - `memberships` with primary key `(team_id, user_id)` and a role.
  - `invitations` with the email normalised and a **partial unique index on `(team_id, email) WHERE status = 'PENDING'`**, so re-inviting replaces the pending invitation.
- [ ] `Role` with `includes(Role)` for the ladder. Use `EnumSet` for allowed-role checks.
- [ ] Use cases: CreateTeam (the creator becomes OWNER), Invite, Revoke, Accept, Decline, ChangeRole, Remove, Leave.
  - Accepting requires: Verified, the invitee's email equals the invitation's, not expired, and not already a teammate.
  - Expiry is computed from `Clock` when an invitation is read (`expiresAt < now` → EXPIRED). No scheduler needed.
  - Inviting returns the same response whether or not the email is registered.
- [ ] Last-OWNER invariant **under concurrency**: two Owners demoting each other at the same moment must not leave zero Owners. Lock the team row (`SELECT … FOR UPDATE` via `@Lock(PESSIMISTIC_WRITE)`) in ChangeRole, Remove and Leave. Test it with two threads and a latch.
- [ ] Authorization:
  - A `TeamAuthorization` bean used from `@PreAuthorize`.
  - **Not a teammate → 404** (don't reveal that the team exists); **teammate with the wrong role → 403**.
- [ ] Retrofit expenses:
  - The `MembershipLookup` adapter now calls `TeamsFacade`; delete the debug role header.
  - Add `teamId` to the MDC.
- [ ] **D5:** `GET /me` in the teams module returns the user, their roles per team and pending invitations matched by verified email.
- [ ] Publish the teams events with `ApplicationEventPublisher` (nobody listens yet).
- [ ] Edge case to test and document: a removed claimant can no longer withdraw or reopen, but approvers can still decide on their expenses.

Tests
- A parameterised **role × endpoint matrix**: no token, non-member, MEMBER, APPROVER, OWNER for every team-scoped endpoint, with the expected status in a table.

### M7 · Receipts

- [ ] Compose: an S3-compatible store (SeaweedFS) and ClamAV. Test them with Testcontainers, or with a file-system `ReceiptStorage` fake plus a fake scanner for fast tests.
- [ ] `V5__receipts`:
  - Columns: `team_id`, `expense_id`, `storage_key`, `original_name`, `detected_type`, `size`, `sha256`, `scan_status`, `possible_duplicate`.
  - **Unique `(expense_id, sha256)`**, so the 409 is race-safe. Index `(team_id, sha256)` for duplicate detection.
- [ ] Ports: `ReceiptStorage` (put, delete, presign) and `MalwareScanner`. Adapters live in `receipts.internal.infrastructure`.
- [ ] Upload (the 5 steps in `CONCEPT.md` §4):
  - Check that the expense is a Draft and the caller is its claimant via `ExpensesFacade`.
  - Stream through a `DigestInputStream` and a counting stream with your own 10 MB cap, in addition to `spring.servlet.multipart.*` limits.
  - Sniff the type with Tika. Generate a random key. Return 202.
- [ ] Scanning:
  - `ReceiptUploaded`, internal to receipts, goes to an `@TransactionalEventListener(AFTER_COMMIT)` + `@Async` scanner.
  - Write your own small ClamAV client (the `INSTREAM` protocol over a socket) and wrap it in `@Retryable`.
  - A clean file becomes ACCEPTED. An infected one becomes QUARANTINED and its object moves to a quarantine prefix.
- [ ] **D3:** receipts implements `ReceiptChecks`, so submitting now really needs an Accepted receipt.
- [ ] Download:
  - Only ACCEPTED receipts, with the same visibility as the expense.
  - A presigned GET valid for ≤ 5 min, with `Content-Disposition: attachment` and a sanitised filename.
- [ ] Delete a receipt (Draft only).
- [ ] When an expense is deleted, a listener on `ExpenseDeleted` removes its receipts and their objects.
- [ ] Note in the learning log: *if the app dies between commit and scan, the receipt is stuck in PENDING_SCAN.* M9 fixes this.

Abuse tests
- An `.exe` renamed to `.pdf` gets 415.
- 10 MB + 1 byte gets 413.
- The EICAR test file becomes QUARANTINED, and downloading it gets 404.
- A team-B receipt id gets 404.
- A filename of `../../etc/passwd` is stored only as metadata.
- Uploading to a SUBMITTED expense gets 409.
- The same file twice on one expense gets 409; on a sibling expense it is flagged as a Possible duplicate.

### M8 · Batch work

- [ ] Language study first (scratch tests, not product code): `ExecutorService`, virtual threads, `CompletableFuture`, a race you cause and then fix, and `ScopedValue`.
- [ ] `POST /teams/{teamId}/expenses:batch`:
  - Validate all ≤ 100 items first.
  - By default, return 200 with per-item `{index, status, id | problem}`. With `atomic=true`, it is all-or-nothing, and a failure returns 422 with per-item problems.
  - Over 100 items → 400. Request-body size is capped by server config.
- [ ] **D4:** `ExpensesFacade.createDrafts(teamId, claimantId, List<NewExpense>)`. It is the one entry point that both batch and import use.
- [ ] Imports:
  - `V6__imports`.
  - The CSV goes to object storage. Move the storage port to `shared` as `ObjectStorage`, which receipts and imports share with separate prefixes.
  - **Before** a job starts, reject a 6th import of the day (429 with `Retry-After`; "today" comes from `Clock`) and files with more than 10,000 rows (pre-count the rows, 400).
- [ ] Spring Batch job:
  - `FlatFileItemReader` → a processor that builds `NewExpense` through the M0 domain factories → a writer that calls `createDrafts`.
  - A skip policy for validation exceptions. A `SkipListener` writes `import_row_errors(line, code, message)`.
  - The job runs asynchronously on a virtual-thread `TaskExecutor`. Test that the job is restartable.
- [ ] `GET …/imports/{jobId}` shows progress and row errors. Only the importer and OWNERs can see it; others get 404. Publish `ImportCompleted`.
- [ ] **D7:** `GET /teams/{teamId}/expenses.csv` neutralises cells that start with `=`, `+`, `-`, `@`, tab or CR (API10).

Tests
- A 10,000-row import with 37 bad rows reports exactly those 37 line numbers.
- An `atomic=true` batch with one bad item writes nothing.
- A 6th import gets 429.

### M9 · Events and audit

- [ ] Add the Modulith JPA starter and `V7__event_publication`. Switch cross-module listeners to `@ApplicationModuleListener`, and turn on republishing of incomplete events on restart. **Re-run the M7 "app dies before scan" scenario** and watch it recover.
- [ ] Give every event a common shape: a module-level sealed interface such as `ExpenseEvent permits ExpenseSubmitted, …`, carrying `actorId`, `teamId` and `occurredAt`.
- [ ] Audit module:
  - `V8__audit` with `details jsonb`. Append-only on two levels: a repository with no update or delete methods, **and** `REVOKE UPDATE, DELETE` in the DB.
  - One listener per event type, idempotent on the event id.
  - Team-less events (`UserRegistered`, `UserVerified`) are stored with `team_id NULL`, so they never appear through the team endpoint.
- [ ] `GET /teams/{teamId}/audit-events` (OWNER only), using the M4 cursor helper.
- [ ] Generate module diagrams with Modulith's `Documenter` into `docs/modules/`.

Tests
- `@ApplicationModuleTest` with the `Scenario` API: submitting an expense → an audit row exists.
- Every event type has a test that it reaches the audit trail.
- A raw SQL `UPDATE audit_events` fails.

### M10 · Hardening and observability

- [ ] Per-user Bucket4j limits returning 429 with `Retry-After`. Move the login limit onto Bucket4j. Buckets are in memory; note in the log what changes with two instances.
- [ ] Security headers:
  - For a JSON API: HSTS, `Content-Security-Policy: default-src 'none'`, `X-Content-Type-Options` and `frame-ancestors 'none'`.
  - A CORS allowlist read from config.
- [ ] Actuator stays on the management port. Add `prometheus` to the exposed endpoints. Swagger UI is off outside `local`.
- [ ] Custom metrics: `ledgerly.receipts.quarantined`, `ledgerly.imports.rows.skipped`, and `ledgerly.expenses.transitions{action}`. Add spans around the S3 and ClamAV calls.
- [ ] ArchUnit (API7): only `..internal.infrastructure..` packages may use HTTP or socket clients.
- [ ] One test class per OWASP row, named `Api1BrokenObjectLevelAuthorizationTest` … `Api10…`, so the `CONCEPT.md` §5 table maps one-to-one onto test files.
- [ ] Log-hygiene test: run the login, refresh, upload and import flows with log capture, then assert that no email, token or password appears.
- [ ] JFR session: record a 10,000-row import and read its allocations and GC with JDK Mission Control. Write the findings in the learning log.

### M11 · Ship it

- [ ] Build the image with `./mvnw spring-boot:build-image` (Paketo runs as non-root). Measure startup with and without the AOT cache and CDS.
- [ ] The `prod` profile fails fast: `@Validated` properties with `@NotBlank` for the DB, keys and S3 credentials.
- [ ] CI runs `verify`, spec lint, gitleaks, an image build, a Trivy scan and a CycloneDX SBOM artifact. Add a `renovate.json`.
- [ ] Keycloak swap:
  - A realm export imported by compose, and a `keycloak` profile that switches off Ledgerly's own token issuing and sets `issuer-uri`.
  - Users are linked to Keycloak by **adding `external_subject` to `users`**, with just-in-time provisioning on the first token.
  - Thanks to ADR 0001, nothing outside identity changes. Prove it with `git diff --stat`.
- [ ] Upgrade to Boot 4.2: read the release notes and run once with `spring-boot-properties-migrator`. Record what broke.

---

## 6. Where people usually get stuck

| Symptom | Likely cause | First thing to check |
| --- | --- | --- |
| `LazyInitializationException` in a controller | An entity leaked out of the transaction | Map it to a DTO inside the use case, and keep `open-in-view=false` |
| A Modulith test fails with "cycle" | A facade call in the wrong direction | §2 arrows; invert with an interface (D3) |
| Money equality is flaky | `BigDecimal.equals` compares scale | Normalise the scale in the `Money` constructor |
| An event listener never fires in a test | `AFTER_COMMIT` listener plus a test that rolls back | Use `@Commit`, the `Scenario` API or `TestTransaction.end()` |
| A 401 in tests after M5 | The debug header has gone | Use the test helper that mints a JWT for a user |
| Paging is slow at 100k rows | Offset paging on a big table | That's expected and why the audit trail is cursor-paged; check `EXPLAIN` |
