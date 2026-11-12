# Black-box parity suite against either implementation

Acceptance is proven by tests that talk only HTTP and can be pointed at any base URL: the vendored Bruno collection, the OWASP API Top 10 tests rewritten as Rust HTTP tests, and response validation against `openapi.yaml`. The suite is written so that it could also run against Ledgerly-Java, but CI only runs it against Rust. Pointing it at Java is an optional manual check (`just parity <base-url>`), for when a test's correctness is in doubt. Java's JUnit tests can't be reused, and rewriting them white-box against Rust internals would prove nothing about parity. Rust keeps its own unit tests and its testcontainers-based integration tests in addition to this suite.

## Setting up state without hooks

Most of Java's tests reach their starting state by writing to the database and moving a fake clock forward, which HTTP alone can't do. The parity suite sets up state the same way for both implementations:

- **SQL fixtures** are written straight into the shared schema: users with real Argon2 hashes, teams, memberships, expenses in any status, accepted receipts, verification tokens inserted as known hashes.
- **Limits and origins** are set through the shared environment variables at startup.
- **A scanner outage** means stopping the clamd container.

Behaviour that needs a fake clock or an injected failure (expiry, lockout, the daily import limit, the unexpected-error 500) is not part of this suite. It is covered by Rust in-process integration tests with an injected `Clock`, ported from the same Java tests. We rejected test-only endpoints in the app (clock control, a verification mailbox, a failing route): one profile mix-up would expose them in production, and Java has no such endpoints, so the suite could not run against Java.
