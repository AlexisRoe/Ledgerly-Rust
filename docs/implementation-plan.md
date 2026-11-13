# Ledgerly-Rust implementation plan

[`CONCEPT.md`](../CONCEPT.md) says **what** Ledgerly-Rust is and how it's built. This plan says **in which order**: milestones M0–M11, numbered and scoped like [Ledgerly-Java's plan](../vendor/ledgerly-java/docs/implementation-plan.md), so the two learning logs can be compared milestone by milestone. Vocabulary follows [`GLOSSARY.md`](../GLOSSARY.md). The working rules are in [`CLAUDE.md`](../CLAUDE.md).

## How to read a step

Every step is one commit and has the same parts:

- **Do:** what to build.
- **Files:** where it goes (paths are targets, not limits).
- **Accept:** the conditions a reviewer checks. All must hold.
- **Java:** where the behaviour is specified. `J/` abbreviates [https://github.com/AlexisRoe/Ledgerly-Java/blob/e2a13909634808b347d30b3dec4c3ffee6741c02/src/main/java/com/ledgerly/](https://github.com/AlexisRoe/Ledgerly-Java/blob/e2a13909634808b347d30b3dec4c3ffee6741c02/src/main/java/com/ledgerly/) and `JT/` the same under `src/test/java/com/ledgerly/`: Ledgerly-Java on GitHub at the pinned commit (the Java code is not in this repository, [ADR 0007](adr/0007-vendor-shared-artifacts-pinned-to-a-java-commit.md)). `R/` is `vendor/ledgerly-java/src/main/resources/`, which is vendored.

Every step ends with a green **`just verify`** (in M0, before the `justfile` has all its parts: `just verify` as far as it exists). Parity tests carry `#[ignore = "M<N>"]` until their milestone. **Un-ignoring the tests of milestone N is part of that milestone's last step.**

Two reference maps save reading time. They're extracts; where they disagree with the vendored code, the code wins:

- [`reference/java-port-spec.md`](reference/java-port-spec.md): facades, events, error catalogue, config properties, crypto and encodings, background flows.
- [`reference/java-test-inventory.md`](reference/java-test-inventory.md): every Java test and Bruno request, with its milestone, whether it can be reproduced black-box, and the **parity traps** (§4).

## Pre-M0 · Design (done)

- [x] Design interview; `GLOSSARY.md` (vendored), ADRs 0005–0013, `CONCEPT.md`, `MISSION.md`, this plan, `CLAUDE.md`, `docs/local-setup.md`, `docs/deployment.md`, `docs/learning-log.md`.
- [x] Ledgerly-Java's shared files vendored at `e2a1390`: `api/openapi.yaml`, `redocly.yaml` and `bruno/` at the root, the rest under `vendor/ledgerly-java/`, with `SOURCE` and the checksum manifest `vendor/ledgerly-java.SHA256SUMS`. The Java code is read on GitHub.
- [x] Demo data extracted to [`reference/demo-data.md`](reference/demo-data.md).
- [x] Reference maps in `docs/reference/`.

---

## M0 · Domain core (no I/O, no async)

**Goal:** Money and the Expense state machine, correct and fully unit-tested, in plain Rust.
**Rust you meet:** ownership and borrowing, structs and enums, exhaustive `match`, `Result` and `thiserror`, newtypes, traits (`Clock`), `rust_decimal`, unit and property tests.

- [x] **M0.1 Workspace and toolchain**
  - Do:
    - Cargo workspace with crates `shared` and `expenses` (libs).
    - `[workspace.lints]`: `unsafe_code = "forbid"`, plus clippy `unwrap_used`, `expect_used`, `panic`, `todo`, `unimplemented`, `indexing_slicing` and `arithmetic_side_effects` all `deny`. `indexing_slicing` and `arithmetic_side_effects` are denied with `#![deny(...)]` at the top of each `domain` module. Tests may `allow` via `#[cfg(test)]`.
    - `rust-toolchain.toml` pinned to the latest stable, with `rustfmt` and `clippy`; edition 2024.
    - `mise.toml` with the tools from `docs/local-setup.md` §2.
    - `justfile` with `verify` (fmt check, clippy, nextest).
    - `.editorconfig`, and a `.gitignore` covering `target/`, `.env`, `.env.*` except `.env.example`, `*.pem`, `*.key`, `.scratch/`.
  - Files: `Cargo.toml`, `rust-toolchain.toml`, `mise.toml`, `justfile`, `crates/{shared,expenses}/`.
  - Accept: `just verify` is green. A deliberately added `unsafe {}` or `.unwrap()` in non-test code fails it (then remove it).

- [x] **M0.2 Vendor drift check**
  - Do: `just vendor-check` verifies every file listed in `vendor/ledgerly-java.SHA256SUMS` (paths relative to the repository root, some with spaces; `shasum -a 256 -c` handles that) and that no unlisted file exists under `vendor/ledgerly-java/`, `api/` or `bruno/`. It works offline and runs first in `just verify`.
  - Files: `justfile`, `scripts/vendor-check.sh`.
  - Accept: editing any vendored byte (including in `api/openapi.yaml` or a Bruno request), or adding a file under `vendor/ledgerly-java/`, `api/` or `bruno/`, makes `just verify` fail with the file's name.

- [x] **M0.3 Typed ids**
  - Do: newtypes `UserId`, `TeamId`, `ExpenseId`, `ReceiptId`, `InvitationId`, `ImportId`, `EventId` over `uuid::Uuid`, with `new_v7()` where Java uses v7 and `new_v4()` where Java uses random v4. Java uses v4 for the refresh-token family, the import id, the storage-key suffix and the JWT `jti` (port spec §3.1). Each newtype has `Display`/`FromStr` and serde as a plain UUID string, and parses only the canonical hyphenated form (decided after M1). No `RejectionId`: rejections have no id in Java or in the schema (decided after M1).
  - Files: `crates/shared/src/id.rs`.
  - Accept: v7 ids created in sequence sort in creation order. A `TeamId` can't be passed where an `ExpenseId` is expected (doc-test with `compile_fail`).
  - Java: `J/shared/id/`.

- [x] **M0.4 Currency table**
  - Do: generate `currencies.rs` from OpenJDK 25's ISO 4217 data (`src/java.base/share/data/currency/CurrencyData.properties` at the `jdk-25-ga` tag, the reference also for later 25.0.x releases (decided after M1); the source URL and tag go in the file header). It maps each code to its default fraction digits exactly as `java.util.Currency.getDefaultFractionDigits()` returns them, including codes Java still accepts that have been withdrawn. Codes with `-1` (XAU, XTS, XXX, …) are present and marked as rejected.
  - Files: `crates/shared/src/money/currencies.rs`, `scripts/gen-currencies.sh`.
  - Accept: EUR 2, JPY 0, KWD 3, CLF 4; XAU and XTS are rejected; the generator is reproducible (`just gen-currencies` produces no diff).
  - Java: `J/shared/money/Money.java`; Java learning log M0.

- [x] **M0.5 Money**
  - Do: `Money { amount: Decimal, currency: Currency }` with a validating constructor:
    - positive (not zero, not negative);
    - scale no greater than the currency's minor units, then normalised to exactly that scale;
    - fits `numeric(19,4)`.
    - `checked_add` and `cmp` across currencies return `Err(CurrencyMismatch)`. `Money` deliberately does not implement `Ord`.
    - Error messages are Java's wording where it reaches clients (port spec §3.2).
  - Files: `crates/shared/src/money/`.
  - Accept: EUR `10.001` is rejected, JPY `1.5` rejected, KWD `1.001` accepted. EUR `10.0` equals EUR `10.00`, and their hashes are equal. `proptest`: any accepted `Money` round-trips through its string form unchanged.
  - Java: `J/shared/money/Money.java`, `JT/shared/money/*`.

- [x] **M0.6 Expense vocabulary**
  - Do: `Category`, `ExpenseStatus`, `Role` (ladder, `includes`), `Actor { id, role }`, `Merchant` and `Note` (length-capped, trimmed per Java; a whitespace-only merchant is rejected), `RejectionReason` (non-blank, ≤ 1000), and the `Clock` trait with `SystemClock` and a test `FixedClock`.
  - Files: `crates/shared/src/{role.rs,clock.rs}`, `crates/expenses/src/domain/`.
  - Accept: unit tests for every cap and every blank case, using the values from the Java tests.
  - Java: `J/expenses/internal/domain/`, `J/shared/` (Role location per port spec §1).

- [x] **M0.7 Expense state machine**
  - Do:
    - `Expense::draft(…, &dyn Clock)`; an `incurred_on` in the future is rejected.
    - The six transitions, taking an `Actor` and returning `Result<_, ExpenseError>` with `InvalidTransition`, `SelfDealing` and `Forbidden`.
    - `submit` takes `ReceiptSummary { accepted, pending_scan }`.
    - Rejections are appended, never overwritten.
    - `edit`, `can_change_receipts` and `can_delete` work only for the claimant on a Draft.
    - `is_visible_to(actor)`.
  - Files: `crates/expenses/src/domain/expense.rs`, `crates/expenses/src/lib.rs` (`ReceiptChecks` trait and `ReceiptSummary`, public per Java ADR 0002).
  - Accept:
    - One table-driven test over 5 statuses × 6 actions: the 6 legal pairs succeed and the other 24 return `InvalidTransition`.
    - Role tests per action, and self-dealing tests for approve, reject and reimburse.
    - All of Java's domain test cases ported (`JT/expenses/internal/domain/`).
  - Java: `J/expenses/internal/domain/Expense.java`.

**Done when:** the definition of done in CONCEPT §8 holds, and the learning log explains why `Money` has no `Ord` and what the exhaustive `match` now guarantees.

---

## M1 · First axum app, tooling, CI, parity-suite skeleton

**Goal:** a running, observable, hardened service skeleton with all tooling and CI in place, and the complete parity suite ported as ignored tests.
**Rust you meet:** `async`/`await`, tokio, axum `Router`/`State`/extractors, tower layers, `tracing`, config parsing into typed structs, crate visibility, multi-stage Docker builds.

- [x] **M1.1 All crates and the architecture test**
  - Do:
    - Create the empty crates `identity`, `teams`, `receipts`, `imports`, `audit`, `api` (placeholder until M3), `app` (binary `ledgerly`) and `parity-tests`, with dependencies exactly as in CONCEPT §3.
    - `crates/app/tests/architecture.rs` reads `cargo metadata` and asserts that the inter-crate dependency graph equals the allowed arrows.
  - Accept: adding `teams → expenses` makes the test fail with a readable message.

- [x] **M1.2 Configuration**
  - Do:
    - Read every environment variable Java reads, under Java's names (port spec §3.9; Java `.env.example`; `application*.yaml`), into typed structs.
    - `SPRING_PROFILES_ACTIVE` selects `local`, `test`, `prod` and `keycloak`, which combine as in Java.
    - `Limits` with Java's defaults.
    - `SPRING_DATASOURCE_URL` accepts `jdbc:postgresql://` and `postgres://`.
    - Secrets are `SecretString`.
    - In `prod`, every missing required variable is collected, and the app exits non-zero listing all of them.
  - Files: `crates/shared/src/config/`, `crates/app/src/config.rs`.
  - Accept: unit tests for each default, for JDBC URL parsing, and for the prod listing (all missing variables in one message). `Debug` of the config shows no secret.
  - Java: `J/shared/config/`, `ProductionEnvironmentCheck`.

- [x] **M1.3 Servers, probes, shutdown**
  - Do:
    - The API listener is on 8080 and management on 8081.
    - Management serves `/actuator/health`, `/actuator/health/liveness`, `/actuator/health/readiness` and `/actuator/info`, with the **same JSON bodies and status codes Spring Boot produces**, and nothing else for now.
    - On SIGTERM, stop accepting connections and drain for up to 30 s.
  - Accept: an integration test starts the app on random ports. Health is `UP`, readiness is `UP`, and an unknown management path gives 404. A shutdown test shows that an in-flight request completes.
  - Java: `R/application.yaml` `management.*`; port spec §3.9; Java deployment doc "Ports and probes".

- [x] **M1.4 Logging and tracing**
  - Do:
    - A `tracing` subscriber with an ECS JSON formatter writing Spring Boot's `ecs` field set (port spec §3.8). In `local`, human-readable output instead.
    - A request span with `traceId` and `spanId` (W3C, 32/16 hex), and OTLP trace export to `MANAGEMENT_OPENTELEMETRY_TRACING_EXPORT_OTLP_ENDPOINT`.
  - Accept: a test captures one log line and asserts its exact key set. The `traceId` in the log equals the one later used in problem bodies (an assertion added in M3).

- [x] **M1.5 Compose and the dev loop**
  - Do:
    - `compose.yaml` with Java's services and digests (`vendor/ledgerly-java/compose.yaml`: postgres, seaweedfs, clamav, lgtm, keycloak), plus a one-shot `flyway` service (pinned by digest) that mounts `vendor/ledgerly-java/src/main/resources/db/migration` read-only and migrates with the app's credentials.
    - An `app` service (profile `app`) built from the Dockerfile, which `depends_on` `flyway` completing successfully. It runs `read_only`, with tmpfs `/tmp`, `cap_drop: ALL` and `no-new-privileges`.
    - Every port on `127.0.0.1`.
    - `just deps`, `migrate`, `run`, `up`, `down`, `reset`, `logs` and `ps`.
    - `.env.example` with Java's keys and comments.
  - Accept: from a clean checkout, `just up` reaches a healthy app, and `docker compose ps` shows only `127.0.0.1` bindings.

- [x] **M1.6 First Dockerfile**
  - Do: the multi-stage build from ADR 0012 (cargo-chef, `cargo auditable build --release`, distroless `cc-debian12:nonroot`, all images pinned by digest). Hardening is finished in M11.
  - Accept: `just image` builds. `docker run … id` is impossible (no shell). The image config's `User` is `65532` or `nonroot`.

- [x] **M1.7 Supply chain and hooks**
  - Do:
    - `deny.toml`: advisories deny; a licence allowlist (MIT, Apache-2.0, BSD-2/3-Clause, ISC, Unicode-3.0, Zlib; extend only with a reason); sources limited to crates.io; ban duplicate major versions (warn).
    - `.pre-commit-config.yaml`: gitleaks, `cargo fmt`, Prettier and Redocly, with versions pinned as in Java.
    - `.gitleaks.toml` that excludes `vendor/` and `bruno/environments/` (Java's development credentials) and nothing else.
    - Prettier and Redocly configuration that never rewrites the vendored files (`.prettierignore`: `api/`, `redocly.yaml`, `bruno/`, `vendor/`). Redocly lints `api/openapi.yaml` with the vendored `redocly.yaml`.
    - `renovate.json` modelled on Java's (digest pinning, a 3-day minimum release age, Cargo, Docker, GitHub Actions).
  - Accept: `just verify` runs `cargo deny check`. A fake AWS key committed outside `vendor/` is caught by the hook.

- [x] **M1.8 CI**
  - Do:
    - `.github/workflows/ci.yml` runs on push to any branch and on pull requests.
    - Jobs: `verify`, `openapi-lint` (vendored spec), `format-config`, `gitleaks` (full history), `deny` and `vendor-drift`.
    - Actions are SHA-pinned, with `permissions: contents: read`, `persist-credentials: false` and `NPM_CONFIG_IGNORE_SCRIPTS=true`.
    - `.github/workflows/upstream.yml` runs weekly and reports (doesn't fail) whether Ledgerly-Java's `main` is ahead of `SOURCE`.
  - Accept: CI is green on a push of this branch, as checked by the reviewer, since the agent doesn't push.

- [x] **M1.9 Parity suite: harness**
  - Do:
    - `crates/parity-tests` holds tests that talk only HTTP and SQL.
      - `PARITY_BASE_URL`: the API under test. `PARITY_DATABASE_URL`: its database, used for fixtures and assertions on stored data.
      - Fixture helpers insert users (real Argon2 hashes, computed once per run), teams, memberships, expenses in any status, rejections, accepted receipts with objects in S3, and verification tokens as known hashes. Every helper is idempotent per test, using fresh UUIDs.
      - A response validator checks every response against the vendored `openapi.yaml`, with `$ref` resolved.
    - **Run groups:** some Java tests need different startup config (OWASP base: `requests-per-minute=250`, `allowed-origins=https://app.example.com`; Api4: 30/min, 12/min for auth; export: `max-export-rows=1200`). Each group declares its environment, and `just parity` starts the app container once per group with that environment, then runs the group's tests. `just parity <url>` runs only the default group against an existing deployment.
    - `just api-test` runs the vendored Bruno collection (pinned Bruno CLI) against a `local` app.
  - Accept: `just parity` runs and prints `passed / ignored / failed` per milestone tag.

- [x] **M1.10 Parity suite: port every test, ignored**
  - Do:
    - For every row in `reference/java-test-inventory.md` §1–§2 marked `BB` or `BB+S`, write the equivalent test, marked `#[ignore = "M<N>"]` with its **Feat** milestone.
    - For each parity trap in §4 (error codes, status precedence, headers, formats, audit content, operational surface), write one explicit test.
    - Rows marked `BB+H` with a `clock` or `fault` hook, and `INT` rows, are **not** parity tests. List them in `crates/parity-tests/NOT_PARITY.md`, each with the milestone and module whose in-process tests will cover it (ADR 0011).
    - One file per OWASP row: `tests/owasp_api<N>_*.rs`.
  - Accept: every inventory row is either a parity test or listed in `NOT_PARITY.md` (a test checks the counts against the inventory). Nothing is un-ignored except what M1 itself makes pass: probes, the management port, unknown-path handling on 8081.

**Done when:** `just up` runs the skeleton in Docker, CI is green, and the parity suite reports its full size, almost all of it ignored.

---

## M2 · Persist expenses

**Goal:** expenses saved and read with sqlx against the schema Flyway built.
**Rust you meet:** sqlx macros and offline mode, executors and transactions (lifetimes in practice), `TryFrom` for row mapping, error conversion with `?`.

- [x] **M2.1 Pool and schema check**
  - Do: create the pool from config. At startup, compare `flyway_schema_history` with the vendored migrations (versions and Flyway checksums). On any difference, exit with a message naming the version.
  - Accept: the app refuses a database missing V10, and one with a tampered checksum. It starts on a freshly migrated one.

- [x] **M2.2 Test database harness**
  - Do: a shared test helper (`shared` crate, `testing` feature) that starts `postgres:17` with testcontainers, runs the pinned Flyway image against it, and hands each test its own transaction or schema state.
  - Accept: two integration tests in parallel don't see each other's rows.

- [x] **M2.3 Expense persistence**
  - Do:
    - `crates/expenses/src/persistence/` with **team-scoped functions only**: `find(team, id)`, `insert`, `update` (with `where version = $n`, `Conflict` when 0 rows are affected), `delete(team, id)`.
    - Map `Money` ↔ `numeric(19,4)` + `char(3)`. Rejections live in `expense_rejections`.
    - Commit `.sqlx/` via `just sqlx-prepare`.
  - Accept:
    - A round trip against real Postgres.
    - The second of two updates that both started from the same version gets `Conflict`.
    - Team A's expense is not found with team B's id.
    - No public function takes an `ExpenseId` without a `TeamId`.

- [x] **M2.4 First use cases**
  - Do: the `CreateExpense` and `GetExpense` application services, each in one transaction, with no web layer yet.
  - Accept: integration tests for both, including validation errors coming out of the M0 constructors.

**Done when:** expenses round-trip through Postgres in integration tests, and `just verify` builds offline (`SQLX_OFFLINE=true`).

---

## M3 · Contract, validation, errors

**Goal:** every expense endpoint except list, batch and export is served through the generated trait, and every error is a Problem Details body identical to Java's.
**Rust you meet:** implementing generated traits, `IntoResponse`, error enums with exhaustive mapping, serde attributes (`deny_unknown_fields`), middleware ordering.

- [ ] **M3.1 Generate the server code**
  - Do:
    - `just generate` runs the pinned `openapitools/openapi-generator-cli` image with the `rust-axum` generator on the vendored spec, writing to `crates/api`. The generated code is committed.
    - `just verify` regenerates into a temp dir and fails on any diff.
    - Spend at most one step's effort evaluating the output. If it can't express the spec (multipart, `:action` paths, problem responses, cookies), **stop** and report it per ADR 0009.
  - Accept: `crates/api` compiles, with lints relaxed for generated code only.

- [ ] **M3.2 Problem Details and the error catalogue**
  - Do:
    - In `shared`, the `ProblemDetail` type and all catalogue entries (port spec §3.3: code, status, type slug, title).
    - One exhaustive mapping from each module's error enum.
    - Rejections Spring produced implicitly: unknown route → 404 `REQUEST_REJECTED`; 405; 415; unreadable JSON and unknown fields → `MALFORMED_REQUEST`; validation → 400 `VALIDATION_FAILED` with `errors[{field,message}]`.
    - A panic-catching layer and unknown errors → 500 carrying only `traceId`.
    - `instance` is the request path.
  - Accept: one test per catalogue entry, comparing the full JSON with Java's shape. The `traceId` in the body equals the one in the log line.
  - Java: `J/shared/error/`, `J/shared/security/ProblemSecurityHandler.java`, inventory §4.1.

- [ ] **M3.3 Current user (no debug headers)**
  - Do:
    - A `CurrentUser` extractor trait in `shared`. Until M5 it has no HTTP implementation: in-process tests construct it directly.
    - **Don't port Java's temporary `X-Debug-*` headers.** They were deleted in Java's M5/M6 and would be a test hook in production code (ADR 0011).
    - `MembershipLookup` port in expenses with a test double (the real one arrives in M6). `ReceiptChecks` test double.

- [ ] **M3.4 Expense endpoints**
  - Do:
    - Create: 201 + `Location`. Get: `ETag: "<version>"`.
    - PATCH and DELETE: `If-Match` required (428 if missing, 412 if stale, `W/"n"` accepted).
    - The six `:action` transitions.
    - Requests are converted into domain types at the edge; `status` and `claimantId` can't be set.
  - Accept: in-process tests (axum `oneshot`) for every status code in the spec for these paths, plus the M3 abuse tests from Java's plan (PATCH with `status` rejected, MEMBER `:approve` 403, self-approval 409, EUR `12.345` 400, future date 400).

- [ ] **M3.5 Idempotency**
  - Do: `Idempotency-Key` on create, using Java's table and semantics (port spec §3.4): same key and same body replays (the expense as it is now); same key and different body → 422; in progress → 409; 24 h expiry. The request hash is **Rust's canonical form** (ADR 0005, accepted exceptions), not Java's `toString()`.
  - Accept: tests for replay, mismatch, concurrency (two tasks with the same key create exactly one expense) and expiry with `FixedClock`.

- [ ] **M3.6 Inventory check**
  - Do: a test asserts that the set of routes the router serves equals the spec's paths and methods (API9). Swagger UI at `/docs` (static, vendored spec) in `local` only.
  - Accept: adding an undocumented route fails the test. In `prod`, `/docs` gets 401 like any other path.

**Done when:** all M3-tagged parity tests that don't need authentication pass in-process. The ones that need real users are tagged M5/M6.

---

## M4 · Paging and filtering

**Goal:** the expense list with Java's paging, filtering and sorting rules, and the cursor helpers for the audit trail.
**Rust you meet:** generics and iterators, `sqlx::QueryBuilder` with bound parameters only, base64/serde for opaque cursors.

- [ ] **M4.1 List endpoint**
  - Do:
    - `page`/`size` (size > 100 → 400, never clamped).
    - `sort` from the allowlist `incurredOn`, `createdAt`, `amount`. Anything else → 400. `amount` without a `currency` filter → 400.
    - Filters: status, category, claimant, currency (an unknown code is 400 even though the spec allows it), `incurredFrom`, `incurredTo`.
    - A MEMBER is forced to `claimant = self`.
    - Response in Spring's `PagedModel` shape.
  - Accept: 250 seeded expenses paged by 100 visit each exactly once; `sort=password` → 400; inventory §4.4 traps for this endpoint.

- [ ] **M4.2 Cursor codec and keyset helper**
  - Do: `Cursor { t: Instant, id: Uuid }` encoded as unpadded Base64url JSON with keys in Java's order (port spec §3.6); junk → 400. A keyset query helper (`(created_at, id) < ($1, $2) … LIMIT n+1`), tested against `expenses`.
  - Accept: inserting rows while paging produces no duplicates. A cursor Java produced decodes (test vector built from the format spec).

**Done when:** M4-tagged parity tests pass, including Bruno `02-reading` list requests (run with fixtures until the M5 seeder exists).

---

## M5 · Authentication and demo data

**Goal:** register, verify, login, refresh with reuse detection, logout, JWT issuing and verification, all interchangeable with Java's stored data.
**Rust you meet:** RustCrypto crates, `secrecy`/`zeroize`, constant-time comparison, tower authentication layers, cookies.

- [ ] **M5.1 Passwords**
  - Do: Argon2id with m=16384, t=2, p=1, a 16-byte salt and a 32-byte hash, as a PHC string in standard Base64 without padding (port spec §4). Login for an unknown email verifies against a dummy hash, so the time is the same; register hashes before checking whether the email exists.
  - Accept: a hash Rust produces parses as Spring's format (`$argon2id$v=19$m=16384,t=2,p=1$…`), and a PHC string built to Spring's documented encoding verifies. Password length is validated before hashing (an over-long password → 400 without hashing).

- [ ] **M5.2 Opaque tokens**
  - Do: 32 random bytes as unpadded Base64url (43 chars), stored as lower-case hex SHA-256. Comparison in constant time.
  - Accept: unit tests for length, alphabet and hash format.

- [ ] **M5.3 Register, verify, login**
  - Do:
    - Register always answers 202 with Java's exact body.
    - Verify: single use, 24 h. Login: the same 401 for an unknown email and a wrong password.
    - The in-memory login lock keyed by email, as Java's V9 design.
    - The `local`-only verification log line, with Java's exact text.
  - Accept: parity tests tagged M5 for these paths. In-process clock tests for lock expiry and verification expiry (ported from `JT/identity/`).

- [ ] **M5.4 Access tokens and JWKS**
  - Do:
    - ES256 tokens; the header has `alg` and `kid`; the claims are only `sub`, `iat`, `exp` (+10 min) and `jti` (v4).
    - The `kid` is the RFC 7638 thumbprint for configured keys, or a random v4 for keys generated in `local`/`test`.
    - Keys are read from PEM or bare Base64 (PKCS#8 private, SPKI public).
    - `GET /.well-known/jwks.json`.
    - Verification: ES256 only, 30 s skew, no issuer or audience check.
  - Accept: tampered, expired, `alg: none`, HS256 and wrong-key tokens → 401. For a fixed key pair, the `kid` equals the thumbprint computed independently in the test.

- [ ] **M5.5 Authentication layer and check order**
  - Do: deny by default; the public allowlist (CONCEPT §4); the 401 problem body. Layer order reproduces Java's precedence: 413 before 401, 401 before routing, and so on (inventory §4.2).
  - Accept: the precedence parity tests tagged M5 pass.

- [ ] **M5.6 Refresh and logout**
  - Do: rotation; reuse revokes the whole family, publishes `RefreshTokenReuseDetected` and logs at warn; a sliding 14-day lifetime; the exact cookie attributes and `Cache-Control: no-store`; `X-Requested-With: ledgerly` required (Java ADR 0004). Logout revokes the family, always answers 204 and clears the cookie.
  - Accept: a replayed refresh token revokes the family. Concurrent refreshes with one token give exactly one success (row lock).

- [ ] **M5.7 Current user from the token**
  - Do: `CurrentUser` from the verified JWT, with `userId` on the request span. Events are defined now and published through a `shared::events::Publisher` trait whose implementation does nothing until M9.

- [ ] **M5.8 Demo data**
  - Do: under `local` and not `test` (and not `keycloak`), a seeder fills an empty database with exactly the data in [`reference/demo-data.md`](reference/demo-data.md), using SQL into the existing tables in one transaction. It skips if the demo team already exists.
  - Accept: `just up` followed by Bruno `00-auth` passes, except the `/me` requests (M6).

**Done when:** M5-tagged parity tests and Bruno `00-auth` (without `/me`) pass, and no log line contains a password, token or cookie (a log-capture test).

---

## M6 · Teams and authorization

**Goal:** teams, invitations, the role ladder and the last-owner rule; every team-scoped endpoint answers 404 or 403 exactly as Java does.
**Rust you meet:** enums with behaviour, trait objects behind `Arc` for facades, database locking under concurrency, two-task tests.

- [ ] **M6.1 Teams use cases**
  - Do: CreateTeam (the creator becomes OWNER), Invite, Revoke, Accept, Decline, ChangeRole, Remove, Leave.
    - Emails are normalised, and inviting gives an identical response whether or not the email is registered.
    - Invitation expiry (7 days) is computed on read.
    - Accepting needs a verified user, a matching email, an unexpired invitation, and a caller who isn't yet a teammate.
  - Accept: parity tests tagged M6 for these paths. Clock tests in-process.

- [ ] **M6.2 Last owner under concurrency**
  - Do: lock the team row (`SELECT … FOR UPDATE`) in ChangeRole, Remove and Leave.
  - Accept: two OWNERs demoting each other at the same moment (two tasks with a barrier) leave exactly one OWNER.

- [ ] **M6.3 Authorization everywhere**
  - Do:
    - `TeamsFacade::role_of` replaces the expenses `MembershipLookup` double.
    - `teams` implements `shared::TeamRoles`.
    - Non-teammate → 404, wrong role → 403, in Java's order (an APPROVER editing someone else's draft → 403).
    - `teamId` goes on the span.
  - Accept: the role × endpoint matrix (no token, non-member, MEMBER, APPROVER, OWNER) for every team-scoped endpoint so far, as one table-driven parity test.

- [ ] **M6.4 `/me`**
  - Do: served by `teams`: the user, roles per team, and pending invitations matched by verified email.
  - Accept: Bruno `00-auth` `/me` requests and `04-teams` pass.

**Done when:** M6-tagged parity tests pass, along with Bruno `01-expenses`, `04-teams` and all of `00-auth`. The removed-claimant edge case is tested and noted in the learning log.

---

## M7 · Receipts

**Goal:** upload, scan, quarantine, download and delete, with Java's exact validation order and messages.
**Rust you meet:** streams and `AsyncRead`, multipart, incremental hashing, `tokio::net::TcpStream`, retries, `'static` tasks and `Send`.

- [ ] **M7.1 Object storage**
  - Do: an `ObjectStorage` trait in `shared` (put, open, delete, move, presign GET), implemented with `aws-sdk-s3` (path-style for SeaweedFS). Ensure the bucket exists at startup. Metric/span `ledgerly.s3{operation}`.
  - Accept: integration test against SeaweedFS in testcontainers.

- [ ] **M7.2 Upload**
  - Do: check the claimant and that the expense is a Draft (via the `ExpensesFacade`) **before** reading the body. Then:
    1. Stream to a temp file through a byte cap and SHA-256.
    2. Sniff the type from magic bytes. Empty → 400, not PDF/JPEG/PNG → 415, over 10 MB → 413.
    3. If the same sha256 is already on this expense → 409 `RECEIPT_DUPLICATE`. If it's on a sibling expense, set `possible_duplicate`.
    4. Store under `receipts/<v4>`.
    5. Insert the receipt as `PENDING_SCAN` and publish the events.
    6. Answer 202, with `Location` pointing at the receipts list (inventory §4.4).

    On failure, delete the object. The temp file is always removed. Display names and download names are sanitised exactly as Java's `FileNames` does.
  - Accept: the M7 abuse list from Java's plan (`.exe` renamed `.pdf`, 10 MB + 1, `../../etc/passwd`, uploading to a SUBMITTED expense, the same file twice).

- [ ] **M7.3 Scanning**
  - Do: a clamd `INSTREAM` client (port spec §5.3: `zINSTREAM\0`, 4-byte length-prefixed chunks, the reply grammar, 2 s connect and 30 s read timeouts). Retries with delays of 500, 1000 and 2000 ms, **reopening the object on each attempt** (fixing Java's quirk, which a client can't observe).
    - Clean → ACCEPTED.
    - Infected → QUARANTINED: key changed to `quarantine/<v4>`, object moved, counter incremented, warn log with the signature.
    - Failure → stays `PENDING_SCAN`.

    Until M9, the scan runs in a task started after commit.
  - Accept: EICAR → QUARANTINED, and its download → 404, against real ClamAV in the parity suite. A scanner outage (container stopped) leaves `PENDING_SCAN`.

- [ ] **M7.4 Submit needs receipts; download; delete**
  - Do: `receipts` implements `expenses::ReceiptChecks`. Download: ACCEPTED only, with the expense's visibility, presigned for 5 minutes, `{url, expiresAt}`, `Content-Disposition: attachment` with the sanitised name. Delete: Draft only. Cleanup on `ExpenseDeleted`.
  - Accept: Bruno `03-lifecycle` passes end to end against the composed stack with real ClamAV.

**Done when:** M7-tagged parity tests pass, along with Bruno `03-lifecycle`. The learning log notes that a crash between commit and scan leaves the receipt stuck until M9.

---

## M8 · Batch work

**Goal:** a 100-item batch, CSV export and an asynchronous 10,000-row CSV import, all with Java's per-item and per-row results.
**Rust you meet:** tasks, `JoinSet`, `Semaphore`, cancellation, `Send`/`Sync` in practice, streaming CSV, back-pressure.

- [ ] **M8.1 Batch create**
  - Do: `POST …/expenses:batch`, at most 100 items (otherwise 400). Every item is validated before any insert. By default the result is partial: 200 with per-item `{index, status, id | problem}`. With `atomic=true` it's all or nothing, and a failure answers 422. Unknown fields in items are rejected. `ExpensesFacade::create_drafts` and `validate` are the single entry points.
  - Accept: an atomic batch with one bad item writes nothing. 101 items → 400.

- [ ] **M8.2 CSV export**
  - Do: `GET …/expenses.csv` with the list's filters and no paging, capped at `max-export-rows` (10,000), with `X-Total-Count` and `X-Truncated`. Java's header row and CRLF line endings. Neutralise cells that start with `=`, `+`, `-`, `@`, tab or CR. Stream the response.
  - Accept: the export parity tests, run in the `max-export-rows=1200` group, and a formula-injection row.

- [ ] **M8.3 Import upload**
  - Do: Java's checks **in Java's order and with its messages** (port spec §5.4):
    1. Teammate check (404 if not).
    2. Daily limit: 5 per user per team per UTC day → 429 with `Retry-After` set to the seconds until UTC midnight.
    3. The size cap.
    4. Strict UTF-8, the header line, no NUL bytes, row count (at most 10,000, at least 1).
    5. Store under `imports/<v4>`.
    6. Under the advisory lock, re-check the daily limit and insert the job `QUEUED` with a v4 id.
    7. Answer 202 with `Location`.
  - Accept: one parity test per message. The 6th import → 429 (a clock test in-process for the UTC-midnight boundary).

- [ ] **M8.4 Import job**
  - Do: a job runner (a tokio task) that processes chunks of 100 rows, each **committed in its own transaction**:
    - Row validation with Java's codes and messages (`COLUMN_COUNT`, `DATE_INVALID`, …, `PARSE_ERROR`).
    - Errors go to `import_row_errors` (`on conflict do nothing`, message truncated to 500 characters) and increment the metric.
    - `imported_rows` grows chunk by chunk, so progress is visible.
    - Status goes `QUEUED → RUNNING → COMPLETED | FAILED`. On success, delete the CSV and publish `ImportCompleted`.

    **Amounts beyond `numeric(19,4)` (decided after M1):** `Money` rejects them as `TooLarge`, a check Java's `Money` lacks. Find out what Java answers for such an import row (presumably a database failure) and make Rust's answer the same; drop or keep `TooLarge` accordingly. Rows with non-ASCII decimal digits import like in Java (`١٢` is 12).

    **Resume after failure (decided 2026-11-12):** on restart of a FAILED job, re-read the CSV and re-validate chunk by chunk **without writing**, until the cumulative imported and skipped counts equal what's stored. Continue with the next chunk. `BATCH_*` tables are not used.
  - Accept: 10,000 rows with 37 bad ones report exactly those 37 line numbers. A crash-injection test (kill the task after chunk k, restart) ends with every valid row imported exactly once.

- [ ] **M8.5 Import status**
  - Do: `GET …/imports/{id}` is visible to the uploader and OWNERs (others get 404). Errors are ordered by line, at most 1000, with `errorsTruncated`.

**Done when:** M8-tagged parity tests pass, including Bruno `02-reading` export.

---

## M9 · Events and audit

**Goal:** a durable outbox in `event_publication`, and an append-only audit trail with Java's event names and details.
**Rust you meet:** enums and trait objects for events, serde tagging, long-running background loops, idempotent consumers.

- [ ] **M9.1 Outbox**
  - Do:
    - `shared::events::Publisher` writes one `event_publication` row per (event, listener) **in the caller's transaction**. `event_type` uses Rust's own names (e.g. `ledgerly-rust:expenses.ExpenseSubmitted`), and the payload is serde JSON.
    - After commit, a dispatcher delivers and sets `completion_date`.
    - Incomplete rows are redelivered: at startup, and every minute for rows older than 2 minutes.
    - Completed rows older than 7 days are purged daily at 03:17.
    - Rust never touches rows whose `event_type` isn't its own (ADR 0005).
  - Accept: a test kills the dispatcher between commit and delivery, restarts it, and sees the event delivered exactly once.

- [ ] **M9.2 Move listeners onto the outbox**
  - Do: receipt scanning, import start and the `ExpenseDeleted` cleanup now go through the outbox.
  - Accept: the M7 "app dies before the scan" scenario recovers on restart.

- [ ] **M9.3 Audit trail**
  - Do: one consumer per event type (all 25), idempotent on the event id (`on conflict`). Java's `eventType` strings and `details` JSON exactly (inventory §4.5). Team-less events have `team_id NULL`. `GET …/audit-events` is OWNER-only via `shared::TeamRoles` and cursor-paged with the M4 helper.
  - Accept: every event type reaches the audit trail with Java's name and details. A raw `UPDATE audit_events` from the app's role fails.

**Done when:** M9-tagged parity tests pass, including Bruno `02-reading` audit.

---

## M10 · Hardening and observability

**Goal:** every OWASP row green, and Java's headers, limits, CORS and metrics exactly.
**Rust you meet:** writing tower layers, `governor`, the `metrics`/Prometheus exporter, profiling (`cargo flamegraph`, `tokio-console`).

- [ ] **M10.1 Rate limits**
  - Do: per user (`requests-per-minute`, 600) on authenticated routes, and per client address (`auth-requests-per-minute`, 60) on the token-free routes, kept in memory and evicted after 2 hours idle. The login limiter is keyed by email. A 429 problem with `Retry-After` **computed as Bucket4j does** (the seconds until the next token; check Java's code). Honour `SERVER_FORWARD_HEADERS_STRATEGY=native`.
  - Accept: Api4 parity tests in their run group (30/min, 12/min for auth).

- [ ] **M10.2 Headers and CORS**
  - Do: Java's exact security headers on every response, including errors and 404s (inventory §4.3). The CORS exact-origin allowlist from `LEDGERLY_SECURITY_ALLOWED_ORIGINS`: a wildcard or a path refuses startup, and a disallowed origin on a real request gets 403 as Java does.
  - Accept: Api8 parity tests and Bruno `05-security`.

- [ ] **M10.3 Metrics and traces**
  - Do: `/actuator/prometheus` on 8081, with Java's metric names and tags (port spec §3.8), plus HTTP server metrics. Spans around S3 and clamd calls. The JVM and Spring MVC series Java's test also asserts (`jvm_memory_used_bytes`, `http_server_requests`) are Java-only; Rust exports `process_resident_memory_bytes` and `http_server_request_duration_seconds` instead.
  - Accept: the MetricsTest equivalents as parity tests that scrape `/actuator/prometheus`.

- [ ] **M10.4 SSRF guard (API7)**
  - Do: the architecture test also asserts that only `infrastructure` modules depend on network-client crates (`reqwest`, `aws-sdk-s3`, `tokio::net` use), and `cargo-deny` bans crates nobody needs.
  - Accept: adding `reqwest` to `expenses::domain` fails the test.

- [ ] **M10.5 Log hygiene**
  - Do: `just parity` captures the app container's logs for the whole run and asserts that no email, password, token, cookie, `Bearer`, merchant/note value, `password_hash` or `$argon2id` appears (inventory: `LogHygieneTest`).
  - Accept: passes. Logging an email on purpose makes it fail.

- [ ] **M10.6 Profiling session**
  - Do: profile a 10,000-row import and a 100-item batch. Record the findings (allocations, lock contention, the slowest query) in the learning log. Fix only what's clearly wrong.

**Done when:** every OWASP parity file is green and only M11 tests remain ignored.

---

## M11 · Ship it

**Goal:** a production-grade image and pipeline, the Keycloak swap, and the full parity suite green.
**Rust you meet:** release profiles (LTO, why `panic = "unwind"` stays: the panic-catching layer from M3.2 depends on it), `cargo auditable`, SBOMs, dependency upgrades.

- [ ] **M11.1 Final image**
  - Do: harden the Dockerfile. Release profile (`lto`, `codegen-units = 1`, `strip`), all digests pinned, no build secrets in layers, a healthcheck via the orchestrator (distroless has no shell, so Compose uses the management port from outside).
  - Accept: the image is under 50 MB, Trivy shows nothing high or critical with a fix, and the container runs `read_only` with no capabilities.

- [ ] **M11.2 Image job in CI**
  - Do: the `image` job builds the image, asserts it runs as non-root, runs Trivy (failing on high or critical with a fix) and uploads a CycloneDX SBOM (`cargo cyclonedx`) as an artifact. (Java documented this job but never implemented it.)
  - Accept: CI is green, as checked by the reviewer after pushing.

- [ ] **M11.3 Keycloak profile**
  - Do: with `keycloak`:
    - Issuer discovery from `SPRING_SECURITY_OAUTH2_RESOURCESERVER_JWT_ISSUER_URI`; the issuer and `aud ∋ ledgerly-api` are required; an unreachable IdP → 401.
    - `/auth/*` and the JWKS endpoint aren't registered.
    - JIT provisioning and linking by `external_subject`, or by verified email, exactly as port spec §4 says.
  - Accept: against the composed Keycloak with the vendored realm, alice logs in through Keycloak and reads her team. A token for another client → 401. Nothing outside `identity` and `app` changed (`git diff --stat`).

- [ ] **M11.4 Production fail-fast review**
  - Do: re-check the `prod` startup refusal against Java's list of required variables, including the `keycloak` combination.
  - Accept: one test per required variable.

- [ ] **M11.5 Dependency-upgrade drill**
  - Do: the counterpart of Java's Boot 4.2 upgrade. Run `cargo update`, take one major-version bump of a core crate (whichever Renovate proposes first), and record what broke in the learning log.

- [ ] **M11.6 Full parity**
  - Do: un-ignore everything that's left. `NOT_PARITY.md` lists only rows covered by in-process tests, each linked to its test.
  - Accept: `just parity` reports 0 ignored and 0 failed, and `just api-test` passes the whole Bruno collection.

**Done when:** CI is green, the image runs as non-root and passes Trivy, login through Keycloak works, and the full parity suite and the Bruno collection pass. After that, the separate lesson-writing agent takes over.

---

## Where agents usually get stuck

| Symptom | Likely cause | First thing to check |
| --- | --- | --- |
| sqlx "query data not found" in CI | `.sqlx/` not regenerated after a query change | `just deps && just sqlx-prepare`, then commit `.sqlx/` |
| The architecture test fails with an unexpected edge | A facade called in the wrong direction | CONCEPT §3 arrows; invert with a trait the lower crate owns (Java ADR 0002) |
| `Money` equality flaky | `Decimal` keeps scale | Normalise the scale in the constructor (M0.5) |
| A parity test passes in-process but fails in `just parity` | Config or layer order differs between the test router and the real app | Build both from the same `app::router(config)` function |
| `future cannot be sent between threads` | A non-`Send` value (e.g. a `MutexGuard`, `Rc`) held across `.await` | Drop it before the `.await`, or use `tokio::sync::Mutex` |
| A problem body differs from Java's in one field | A Spring default the spec doesn't mention | `reference/java-test-inventory.md` §4; then the Java handler code |
| Receipts stuck in `PENDING_SCAN` locally | ClamAV still loading signatures (minutes; slower emulated) | `docker compose logs clamav` |
