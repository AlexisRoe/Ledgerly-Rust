# Learning Java & Spring Boot: The Ledgerly Backend

Nov 4, 2026 · @Alexis

## 1. The project: Ledgerly

Ledgerly is a REST backend where small teams record expenses and attach receipt files. It is small enough to finish in a few months of evenings, yet every requirement you listed falls out of the domain naturally instead of being bolted on.

What a user can do:

- Register, verify an email address, log in, refresh a session, log out. Belong to one or more teams with a role: OWNER, APPROVER or MEMBER (a ladder: each role includes the ones below it). Join a team by accepting an invitation; leave it again.
- Ubiquitous language lives in [`GLOSSARY.md`](GLOSSARY.md), significant decisions in [`docs/adr/`](docs/adr/), and the build order in [`docs/implementation-plan.md`](docs/implementation-plan.md). Where this document and the glossary disagree, the glossary wins.
- Create expenses (amount, currency, category, date, merchant, note) and attach receipts (PDF, JPEG, PNG).
- Browse and filter expenses page by page, sorted by date or amount, or export them as CSV.
- Create up to 100 expenses in one request, or import a CSV of thousands of rows as a background job with a status endpoint.
- Move an expense through a workflow: DRAFT → SUBMITTED → APPROVED → REIMBURSED. A submitted expense can be withdrawn back to DRAFT by its claimant or REJECTED (with a reason) by an approver; a rejected expense can be reopened as a DRAFT. APPROVED cannot be undone, REIMBURSED is final.
- Read an audit trail of who did what, when.

| You asked for | Where Ledgerly needs it |
| --- | --- |
| Authentication | Login with short-lived JWT access tokens, rotating refresh tokens, team roles checked on every call |
| File upload | Receipts: multipart upload, type sniffing, size limits, private object storage, time-limited download links |
| Batch requests | `POST /expenses:batch` with per-item results, plus an asynchronous CSV import job |
| Page requests | Offset paging for expense lists, cursor (keyset) paging for the audit trail |
| Error handling | Validation, not found, forbidden, version conflicts, all as RFC 9457 Problem Details |
| Logging | Structured JSON logs with trace ids, a separate audit log, nothing secret or personal in either |
| OpenAPI | A contract-first spec in the repo, served as docs in dev, checked in CI |

Money is a deliberately good teacher here: it forces you to learn `BigDecimal` over `double`, immutability and value objects early. Out of scope: a frontend, real payments and email delivery (emails are a logged event).

## 2. Tech stack (November 2026)

Build on Java 25 LTS and Spring Boot 4.1.x, then upgrade to 4.2 once it goes GA. Spring Boot 4.2.0-RC1 [shipped on 23 October 2026](https://adtmag.com/articles/2026/10/30/spring-boot-4dot2-release-candidate-targets-startup-performance-and-upgrade-checks.aspx), and doing that minor upgrade yourself is a useful lesson in reading release notes.

| Concern | Choice | Why this one |
| --- | --- | --- |
| Language | Java 25 (LTS), Eclipse Temurin build | Current LTS; records, sealed types, pattern matching and virtual threads are all final |
| Framework | Spring Boot 4.1.x (Spring Framework 7, Jakarta EE 11) | [Recommended target for new projects](https://www.herodevs.com/blog-posts/spring-boot-versions-eol-dates-and-latest-releases-april-2026); 3.5 left OSS support in June 2026 |
| Build | Maven with the wrapper (`mvnw`) | Most common in industry, declarative, easiest to read as a beginner; Gradle is a fine later swap |
| Web | Spring MVC on virtual threads | Simple blocking code that still scales; skip WebFlux until you need streaming |
| Database | PostgreSQL, Spring Data JPA (Hibernate 7), Flyway migrations, `JdbcClient` for reports | Industry default; migrations make schema changes reviewable |
| Modularity | Spring Modulith | Enforces module boundaries in tests, gives you event publication with an outbox |
| Security | Spring Security 7, OAuth2 Resource Server (JWT), Argon2id hashing | Standards-based; later swap your own login for an OIDC provider (Keycloak) without touching the API |
| Files | S3-compatible object storage behind a `ReceiptStorage` interface, Apache Tika, ClamAV | Real upload hygiene: type sniffing, malware scanning, private buckets |
| Batch | Spring Batch 6 for CSV import | Restartable jobs, chunking, skip policies, job status for free |
| Resilience | Spring Framework 7 `@Retryable` and `@ConcurrencyLimit`, Bucket4j rate limits | Built in now, no extra resilience library needed for this size |
| API contract | OpenAPI 3.1 file, openapi-generator (interfaces only), [springdoc-openapi 3.x](https://github.com/springdoc/springdoc-openapi) to serve docs | Contract-first is the 2026 norm for teams; springdoc v3 is the line for Boot 4 |
| Logging and telemetry | SLF4J + Logback structured JSON, Micrometer, OpenTelemetry | Boot writes structured logs natively; traces tie logs to requests |
| Testing | JUnit, AssertJ, Mockito, Testcontainers, `RestTestClient`, Spring Modulith tests | Real Postgres in tests; boundaries verified automatically |
| Code quality | JSpecify null annotations + NullAway, Spotless formatter, Renovate, Trivy, gitleaks | Null safety and supply-chain hygiene from day one |

Pin exact patch versions when you generate the project at start.spring.io; they move every few weeks.

## 3. Architecture: a modular monolith

Ledgerly is one deployable Spring Boot app split into six business modules that talk only through public facades and events. It is how most teams start in 2026: microservice-style boundaries without network calls, and each module could be extracted later.

&#91;embedded content: Ledgerly architecture · 6 layers in a module, 6 modules, 1 outbox\]

The left column is one call inside the highlighted expenses module; the right shows that modules never call each other to write, so audit learns about changes only through stored events.

| Module | Owns | Publishes |
| --- | --- | --- |
| identity | Users, password hashes, email verification, refresh tokens, login and logout | UserRegistered, UserVerified |
| teams | Teams, memberships, roles, invitations | InvitationSent, InvitationRevoked, TeammateJoined, TeammateLeft, TeammateRemoved, RoleChanged |
| expenses | Expenses, the approval workflow, batch create, list queries | ExpenseSubmitted, ExpenseWithdrawn, ExpenseApproved, ExpenseRejected, ExpenseReopened, ExpenseReimbursed, ExpenseDeleted |
| receipts | Upload, scanning, storage keys, download links | ReceiptAccepted, ReceiptQuarantined |
| imports | CSV import jobs (Spring Batch) and their status | ImportCompleted |
| audit | Append-only audit trail built from everyone's events | nothing |
| shared | Money, typed ids, error catalogue, security helpers; no business rules | nothing |

Three rules keep the boundaries honest:

1. A module exposes only its top-level package; everything under `internal` is private. A Spring Modulith verification test fails the build on any violation.
2. Never touch another module's tables, repositories or `internal` classes. Another module may send a command through a facade (imports asking expenses to create drafts) or read through one (expenses asking teams whether a user is an APPROVER); everything else reacts to events ([ADR 0003](docs/adr/0003-commands-through-facades-not-only-events.md)). Dependencies never form a cycle: when two modules need each other, the lower one owns an interface and the higher one implements it, as expenses does with `ReceiptChecks` ([ADR 0002](docs/adr/0002-expenses-asks-receipts-through-an-interface-it-owns.md)).
3. Events are stored in Modulith's event publication registry (a transactional outbox), so a crash after commit never loses them.

Inside each module, four layers (shown for expenses):

```text
com.ledgerly.expenses
├── ExpensesFacade          public: the only class other modules may call
├── events/                 public: event records such as ExpenseSubmitted
└── internal/
    ├── web/                controllers implementing the generated OpenAPI interfaces, DTO mappers
    ├── application/        use cases: SubmitExpense, ApproveExpense, BatchCreateExpenses
    ├── domain/             Expense aggregate, ExpenseStatus, approval policy, Money
    └── persistence/        Spring Data repositories, query specifications
```

A pragmatic choice: domain objects are JPA entities that carry their own behaviour (a rich model), while web DTOs stay separate records. Splitting persistence models from domain models (full hexagonal) is a stretch exercise, not the default.

## 4. Domain model and API surface

Seven aggregates and about thirty endpoints, every business URL scoped under a team so authorization has one obvious anchor.

| Aggregate | Key fields | Teaches |
| --- | --- | --- |
| User | id, email, Argon2id hash, verified, createdAt, locked | Value objects (Email), immutability |
| Team, Membership | team id, user id, role (OWNER, APPROVER, MEMBER); at least one OWNER per team | Enums with behaviour, many-to-many done right, aggregate-wide invariants |
| Invitation | id, teamId, email, role, invitedBy, expiresAt (7 days), status (PENDING, ACCEPTED, DECLINED, REVOKED, EXPIRED) | Lifecycles driven by an injected `Clock` |
| Expense | id, teamId, claimantId, Money (amount + ISO currency), category (fixed enum), incurredOn, merchant, note, status, rejection reasons, version | Rich domain model, state machine, optimistic locking |
| Receipt | id, expenseId, storage key, detected type, size, SHA-256, scan status (PENDING\_SCAN, ACCEPTED, QUARANTINED), possible duplicate | I/O streams, hashing, external systems |
| ImportJob | job id, file key, status, rows read, written, skipped | Asynchronous work, Spring Batch metadata |
| AuditEvent | id, occurredAt, actor, team, type, JSON details | Append-only data, event listeners |

Ids are UUIDv7: time-ordered for index friendliness and not guessable. Every record outside identity refers to users by id only, never by email or name ([ADR 0001](docs/adr/0001-users-referenced-by-id-only.md)).

Expense rules worth encoding in the domain:

- `Money` refuses to add amounts in different currencies; a team may mix currencies, so totals are always grouped by currency. Amounts are positive with no more decimals than the currency's minor units (EUR 2, JPY 0, KWD 3).
- `incurredOn` cannot be in the future; merchant is required, note optional, both length-capped.
- Only a DRAFT can be edited, have receipts added or removed, or be deleted (by its claimant).
- Submitting needs at least one ACCEPTED receipt and no receipt still PENDING\_SCAN.
- No self-dealing: nobody approves, rejects or reimburses an expense they claimed. A one-person team therefore never gets approvals, and that is intended.
- MEMBERs see only their own expenses; APPROVERs and OWNERs see all of the team's. A removed teammate's expenses stay and continue through the workflow.

| Transition | From → to | Who |
| --- | --- | --- |
| submit | DRAFT → SUBMITTED | claimant |
| withdraw | SUBMITTED → DRAFT | claimant |
| approve | SUBMITTED → APPROVED | APPROVER or OWNER, not the claimant |
| reject (reason required) | SUBMITTED → REJECTED | APPROVER or OWNER, not the claimant |
| reopen | REJECTED → DRAFT | claimant |
| reimburse | APPROVED → REIMBURSED | OWNER, not the claimant |

| Endpoint (all under `/api/v1`) | Purpose | Pattern it teaches |
| --- | --- | --- |
| `POST /auth/register`, `/auth/verify`, `/auth/login`, `/auth/refresh`, `/auth/logout` | Account and session lifecycle | Refresh token in an HttpOnly, Secure, SameSite=Strict cookie; access token in the body; verification link delivered as a logged event |
| `GET /me` | Current user, team roles, pending invitations (served by the teams module) | Reading the security context |
| `POST /teams` | Create a team; the creator becomes its OWNER | Role-based method security |
| `POST /teams/{teamId}/invitations`, `DELETE …/invitations/{id}` | Invite or revoke, OWNER only | Identical response whether or not the email is registered |
| `POST /invitations/{id}:accept`, `:decline` | Invitee answers; accepting needs a verified email | Expiry with an injected `Clock` |
| `PATCH`, `DELETE /teams/{teamId}/members/{userId}`, `POST /teams/{teamId}:leave` | Change role, remove, leave | Last OWNER can never leave or be demoted |
| `GET /teams/{teamId}/expenses` | Filtered, sorted, paged list | Offset paging: `page`, `size` (max 100), `sort` from an allowlist; filters status, category, claimant, currency, incurred-on range; sort by amount only with a currency filter |
| `GET /teams/{teamId}/expenses.csv` | Export the filtered list, up to 10,000 rows | Streaming responses; cells starting with `=`, `+`, `-`, `@` neutralised |
| `POST /teams/{teamId}/expenses` | Create one expense | `Idempotency-Key` header, 201 + `Location` |
| `GET`, `PATCH`, `DELETE /teams/{teamId}/expenses/{id}` | Read, edit and delete (DRAFT only) | `ETag` / `If-Match` for lost-update protection, 412 on mismatch |
| `POST /teams/{teamId}/expenses:batch` | Create up to 100 at once | Per-item results; `atomic=true` switches to all-or-nothing |
| `POST …/expenses/{id}:submit`, `:withdraw`, `:approve`, `:reject`, `:reopen`, `:reimburse` | Workflow transitions | Custom action methods, 409 on illegal transitions |
| `POST …/expenses/{id}/receipts`, `DELETE …/receipts/{receiptId}` | Multipart upload, removal (DRAFT only) | 202 Accepted, status PENDING\_SCAN; 409 for the same file twice on one expense |
| `GET …/receipts/{receiptId}/download` | Fetch a receipt | Short-lived presigned link, never a public bucket |
| `POST /teams/{teamId}/imports`, `GET …/imports/{jobId}` | CSV import and progress; visible to the importer and OWNERs | 202 + `Location` polling pattern |
| `GET /teams/{teamId}/audit-events` | Audit trail, OWNER only | Cursor (keyset) paging: opaque `after` cursor, `limit` |

Two paging styles on purpose. Offset paging returns `items` plus `page` metadata (number, size, totalElements, totalPages) using Spring Data's stable `PagedModel` shape; it suits UI tables. Cursor paging returns `items` plus `nextCursor`; it stays fast and consistent on a table that only grows.

Batch and import always create DRAFT expenses with the caller as claimant. Imports are capped at 5 per user per team per day and 10,000 rows per file; invalid rows are skipped and reported by line number. Batch semantics are a design decision you document in the spec: default is partial success, where the response lists each item's index, outcome, new id or a problem object. Validation runs on all items before any insert; the 100-item cap and a request-size limit protect the server.

Receipt upload, step by step:

1. Stream the multipart part to a temp file while computing SHA-256 and enforcing a 10 MB cap. Same hash already on this expense → 409; same hash on another expense in the team → mark as possible duplicate for approvers.
2. Detect the real type from magic bytes with Tika; reject anything outside PDF, JPEG, PNG regardless of the declared type or file name.
3. Store under a random key in a private bucket; the original name is metadata only.
4. Publish ReceiptUploaded (internal to the receipts module); a listener sends the file to ClamAV.
5. Clean → ACCEPTED and downloadable. Infected → QUARANTINED, audit event, never served.

## 5. Security by design

Security is designed in from milestone 1: deny by default, every rule has a test that tries to break it, and the [OWASP API Security Top 10 (2023)](https://owasp.org/API-Security/editions/2023/en/0x11-t10/) is the checklist.

Principles that shape every module:

- **Deny by default.** The security filter chain permits only `/auth/**`, health probes and (in dev) the API docs; everything else needs a valid token.
- **Tenant scoping in the data layer.** Every repository query takes `teamId`, and membership is checked before the query runs. There is no "find expense by id" without a team.
- **Explicit input and output shapes.** Requests and responses are records generated from the spec; entities never leave the module, so mass assignment is impossible.
- **Limits everywhere.** Page size, batch size, upload size, request body size, login attempts, requests per user per minute.
- **Secrets never in Git.** Config comes from environment variables; `.env` is git-ignored, `.env.example` documents keys, gitleaks runs as a pre-commit hook and in CI.

| OWASP API risk | Ledgerly control | Test that proves it |
| --- | --- | --- |
| API1 Broken object level authorization | Team-scoped queries; membership check in every use case; MEMBERs scoped to their own expenses | User from team A gets 404 for team B's expense and receipt; MEMBER gets 404 for a teammate's expense |
| API2 Broken authentication | Argon2id; 10-minute asymmetric-signed JWTs; refresh rotation with reuse detection; login rate limit and lockout; verified email required to accept invitations | Replayed refresh token revokes the whole session family; unverified account cannot accept an invitation |
| API3 Broken object property level authorization | Separate request and response records; `status` and `claimantId` not writable | PATCH with `status: APPROVED` is ignored or rejected |
| API4 Unrestricted resource consumption | Max page 100, batch 100, upload 10 MB, Bucket4j per-user limits | 101-item batch returns 400; burst returns 429 with `Retry-After` |
| API5 Broken function level authorization | `@PreAuthorize` with team role checks on use cases | MEMBER calling `:approve` gets 403 |
| API6 Sensitive business flows | No self-dealing (approve, reject, reimburse own expense); invitations never reveal registered emails; import capped at 5 per day | Self-approval and self-reimbursement return 409; inviting a registered and an unknown email give identical responses |
| API7 SSRF | No feature fetches user-supplied URLs; outbound HTTP only to an allowlist | ArchUnit rule: only `infrastructure` may use HTTP clients |
| API8 Security misconfiguration | Security headers, CORS allowlist, Actuator on a separate port, Swagger UI off outside dev, no stack traces in errors | Prod-profile test asserts `/swagger-ui` is 404 |
| API9 Improper inventory management | The OpenAPI file is the inventory; versioned paths; CI fails on undocumented endpoints | Contract test compares served spec with the file |
| API10 Unsafe consumption of APIs | CSV and ClamAV responses treated as untrusted; CSV cells starting with `=`, `+`, `-`, `@` neutralised on export | Formula-injection row is escaped |

Authentication is taught in two stages. First you build it: Ledgerly's identity module hashes passwords, issues JWTs signed with a private key and publishes the public key at a JWKS endpoint. Then you replace the issuer with Keycloak over OpenID Connect; because the rest of Ledgerly is only an OAuth2 resource server, nothing outside identity changes. Passkeys (WebAuthn) are a stretch goal.

Supply chain and runtime: Renovate keeps dependencies current, Trivy scans the image, Boot's CycloneDX SBOM is published with each build, and the container runs as a non-root user on a minimal base image.

## 6. Errors, logging, observability and OpenAPI

Every error leaves Ledgerly as an RFC 9457 Problem Details response, every log line is JSON tied to a trace id, and the OpenAPI file is written before the code.

### Error handling

Domain code throws a small sealed hierarchy of exceptions (not found, forbidden, conflict, rule violated); one `@RestControllerAdvice` turns each into a `ProblemDetail`. Unknown exceptions become a generic 500 that carries only the trace id; the stack trace stays in the log.

| Problem field | Content | Example value |
| --- | --- | --- |
| `type` | Stable URI from the error catalogue | `https://ledgerly.dev/problems/invalid-transition` |
| `title`, `status` | Human summary, HTTP status | Illegal workflow transition, 409 |
| `detail` | Safe, specific message | Expense is APPROVED and cannot be submitted |
| `instance` | The request path | `/api/v1/teams/…/expenses/…:submit` |
| `code` (extension) | Machine-readable code clients switch on | `EXPENSE_INVALID_TRANSITION` |
| `traceId` (extension) | Links the response to logs and traces | 32 hex characters |
| `errors` (extension) | Per-field validation messages | field `amount`, must be positive |

The catalogue lives in the spec as reusable responses, so clients see every error a call can return. Authentication errors never reveal whether an email exists.

### Logging

Use Boot's built-in structured logging (ECS JSON) in every profile except a human-readable local console. Micrometer Tracing puts `traceId` and `spanId` on every line; a filter adds `userId` and `teamId` (internal ids, never emails).

| Level | Use for |
| --- | --- |
| ERROR | A request or job failed and someone should look |
| WARN | Something suspicious that recovered: rate limit hit, quarantined file, refresh-token reuse |
| INFO | Business milestones: expense approved, import finished |
| DEBUG | Developer detail, off outside local |

Never logged: passwords, tokens, cookies, `Authorization` headers, file contents, full request bodies. The audit trail is separate: an append-only table written from domain events, answering "who changed what", not a log file.

### Observability

Micrometer metrics (HTTP latency, plus custom counters such as receipts quarantined and import rows skipped) and OpenTelemetry traces export over OTLP to a local Grafana stack. Liveness and readiness probes come from Actuator on a separate management port.

### OpenAPI, contract first

1. Change `api/openapi.yaml` first: paths, schemas, examples, error responses, the bearer security scheme.
2. Lint it with Redocly or Spectral (part of the build).
3. The build generates Java interfaces and request/response records; you implement the interfaces.
4. springdoc serves the docs and Swagger UI in the dev profile only.
5. A CI contract test fails if the served spec drifts from the file.

## 7. Local repository setup

One Git repository, one Maven project, one `compose.yaml`: starting the app from the IDE brings up every dependency automatically through Spring Boot's Docker Compose support, and tests use Testcontainers so they never touch your dev data.

| Install | Purpose |
| --- | --- |
| JDK 25 (Temurin) via SDKMAN or mise | Switch JDKs per project; a `.sdkmanrc` or `mise.toml` pins the version |
| IntelliJ IDEA (or VS Code with the Java extension pack) | Debugger, refactoring, Spring tooling |
| Docker Desktop, OrbStack or Podman | Runs Postgres and friends, and Testcontainers |
| Git with the pre-commit framework | Runs gitleaks and the formatter before each commit |
| Bruno or IntelliJ's HTTP client | Request collections committed to the repo |

```text
Ledgerly-Java/                  the Maven project lives at the repository root
├── .github/workflows/ci.yml     build, tests, spec lint, gitleaks, Trivy
├── .mvn/, mvnw, mvnw.cmd        Maven wrapper: no global Maven needed
├── api/openapi.yaml             the contract, edited before any code
├── compose.yaml                 local dependencies
├── docs/
│   ├── adr/                     one short decision record per significant choice
│   ├── implementation-plan.md   build order, step by step
│   └── learning-log.md          what you learned each session, questions open
├── http/                        request collections per module
├── src/main/java/com/ledgerly/
│   ├── identity/  teams/  expenses/  receipts/  imports/  audit/  shared/
│   └── LedgerlyApplication
├── src/main/resources/
│   ├── application.yaml         safe defaults, no secrets
│   ├── application-local.yaml   human-readable logs, Swagger UI on
│   └── db/migration/            Flyway scripts V1__, V2__ …
├── src/test/java/com/ledgerly/  unit, slice, integration, architecture tests
├── CONCEPT.md, GLOSSARY.md      this document and the ubiquitous language
├── .env.example                 every variable documented, no real values
├── .editorconfig, .gitignore, .pre-commit-config.yaml
├── pom.xml
└── README.md                    how to run, test, and where to start reading
```

| Compose service | Role | Added in milestone |
| --- | --- | --- |
| PostgreSQL | Main database | 2 |
| Grafana OTel LGTM (single image) | Logs, metrics, traces UI | 1 |
| S3-compatible store (for example SeaweedFS) | Receipt and CSV storage | 7 |
| ClamAV | Malware scanning | 7 |
| Mailpit | Catches outgoing mail (password reset stretch goal) | 9 |
| Keycloak | External OIDC provider | 11 |

Profiles: `local` for your machine, `test` for automated tests, `prod` for the container image; `prod` refuses to start if required secrets are missing. Day to day you need three commands: run the app from the IDE, `./mvnw verify` before pushing, and `docker compose down -v` to reset local data.

Git habits worth copying from industry: protect `main`, one branch and pull request per milestone, Conventional Commits, a PR template with a short security checklist, and a tag at the end of each milestone so you can diff your progress.

## 8. Learning path

Twelve milestones, roughly 18 to 22 weeks at 6 to 8 hours a week; each pairs a Java language topic with the Spring feature that needs it, so you learn the language exactly when the framework makes you want it. Milestone 0 has no Spring at all, on purpose: you should understand plain Java before a framework starts doing things for you.

| # | Milestone (rough weeks) | Java you learn | Spring you learn | Done when | Status |
| --- | --- | --- | --- | --- | --- |
| 0 | Plain-Java core (2) | Classes, interfaces, records, enums, `equals`/`hashCode`, immutability, `BigDecimal`, `java.time`, collections, exceptions, JShell, JUnit + AssertJ | None | `Money` (currency-safe, minor-unit scale) and the full expense state machine (all six transitions, self-dealing rule) work, fully unit-tested, no framework | Not started |
| 1 | First Boot app (1) | Packages and visibility, annotations, what reflection is, records as config | Starters, auto-configuration, dependency injection, profiles, `@ConfigurationProperties`, Actuator, structured logs, Modulith verification test | App starts, health is UP, JSON logs show in Grafana, module boundaries checked in the build | Not started |
| 2 | Persist expenses (2) | `Optional`, lambdas, streams, entity identity vs value equality | Spring Data JPA, Flyway, transactions, Testcontainers | Expenses saved and read in an integration test against real Postgres | Not started |
| 3 | Contract, validation, errors (2) | Sealed hierarchies, pattern-matching `switch`, nested records | OpenAPI generator, Bean Validation, `ProblemDetail`, `@RestControllerAdvice`, `RestTestClient` | Every endpoint in the spec, every error a Problem Details response | Not started |
| 4 | Paging and filtering (1) | Generics and wildcards, functional interfaces, `Comparator`, Base64 | `Pageable`, sort allowlists, specifications, keyset queries with `JdbcClient` | Offset paging on expenses and the cursor helpers (tested against expenses, wired to the audit trail in 9) work; page size capped | Not started |
| 5 | Authentication (2) | `Instant`, `Duration`, injecting a `Clock`, `SecureRandom`, key pairs | Security filter chain, password encoders, JWT issuing and resource server, cookies, CSRF reasoning | Login, refresh rotation with reuse detection, logout, all tested | Not started |
| 6 | Teams and authorization (1–2) | `EnumSet`, enums with behaviour, method references | Method security, `@PreAuthorize`, custom authorization managers | Invitations, role ladder and last-OWNER rule work; cross-team and wrong-role tests return 404 and 403 | Not started |
| 7 | Receipt upload (2) | `InputStream`, try-with-resources, NIO `Path` and `Files`, `MessageDigest` | Multipart handling, size limits, ports and adapters for storage, application events | Upload, scan, quarantine and download-link flows tested | Not started |
| 8 | Batch work (2) | Threads, `ExecutorService`, virtual threads, `CompletableFuture`, scoped values, thread safety | Batch endpoint design, Spring Batch jobs, `@Async`, `@Retryable` | 100-item batch and 10,000-row CSV import both work and report per-row errors; CSV export neutralises formulas | Not started |
| 9 | Events and audit (1) | Observer pattern, sealed event types | Spring Modulith event publication registry, `@ApplicationModuleListener` replacing plain `@TransactionalEventListener` | Modulith verification passes; audit trail shows every change | Not started |
| 10 | Hardening and observability (1–2) | Heap, garbage collection, JFR, `jcmd` | Rate limiting, security headers, CORS, custom metrics, tracing | OWASP table in section 5 has a passing test per row | Not started |
| 11 | Ship it (1–2) | Build lifecycle, AOT cache and CDS for startup | Buildpacks image, CI pipeline, Keycloak OIDC swap, upgrade to Boot 4.2 | Green CI, container runs as non-root, login via Keycloak works | Not started |

After milestone 11, pick stretch goals: passkeys, a full hexagonal split of one module, a GraalVM native image, or extracting `imports` into its own service to feel what the monolith saved you from.

## 9. How to study

You write every line of code yourself; the design above is the spec, and each milestone runs through the same seven-step loop.

1. **Decide first.** Write the OpenAPI change or a half-page decision record before touching Java.
2. **Learn the Java topic in isolation.** Ten to thirty minutes in JShell or a scratch test on the milestone's language topic, using the [dev.java tutorials](https://dev.java/learn/).
3. **Read the Spring reference section** for the feature, not a random blog post. Note what Boot auto-configures for you and how to see it (the conditions report in Actuator).
4. **Test first.** A failing test that describes the behaviour, then the code that passes it.
5. **Attack it.** Write the abuse test from the section 5 table: wrong team, wrong role, oversized input, replayed token.
6. **Explain it back.** Five sentences in `docs/learning-log.md`: what you built, why Spring needed that Java concept, one thing still unclear.
7. **Get reviewed.** Open a pull request and ask Claude to review the diff for idioms, security and tests, or to quiz you on the milestone. Ask for hints, not solutions; if stuck for more than 30 minutes, ask for the next smallest hint.

Definition of done for every milestone:

- [ ] `./mvnw verify` is green, including Modulith and architecture tests
- [ ] The OpenAPI file matches what the app serves
- [ ] At least one abuse test per new endpoint
- [ ] No secrets, tokens or personal data in logs or Git history
- [ ] Learning log entry written, milestone tagged

References worth keeping open: the [Spring Boot reference](https://docs.spring.io/spring-boot/), the [Spring Security reference](https://docs.spring.io/spring-security/reference/), the [Spring Modulith reference](https://docs.spring.io/spring-modulith/reference/), the [OWASP File Upload Cheat Sheet](https://cheatsheetseries.owasp.org/cheatsheets/File_Upload_Cheat_Sheet.html) and [RFC 9457](https://www.rfc-editor.org/rfc/rfc9457). For books, Effective Java (3rd edition) still teaches the language's core idioms better than anything newer.

## Sources

- [Spring Boot 4.2 release candidate, ADTmag, 30 Oct 2026](https://adtmag.com/articles/2026/10/30/spring-boot-4dot2-release-candidate-targets-startup-performance-and-upgrade-checks.aspx)
- [Spring Boot versions and EOL dates, HeroDevs, July 2026](https://www.herodevs.com/blog-posts/spring-boot-versions-eol-dates-and-latest-releases-april-2026)
- [springdoc-openapi on GitHub](https://github.com/springdoc/springdoc-openapi)
- [OWASP API Security Top 10, 2023](https://owasp.org/API-Security/editions/2023/en/0x11-t10/)
