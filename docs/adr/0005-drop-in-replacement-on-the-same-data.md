# Drop-in replacement on the same data

Ledgerly-Rust implements the same HTTP contract, database schema and behaviour as Ledgerly-Java at the vendored commit, closely enough that either implementation can be pointed at a database, object store and signing key written by the other and serve the same users without migration. We chose this over "equivalent, separate databases" because it makes parity falsifiable: any divergence shows up as data one side cannot read.

## Consequences

Everything Java derived from Spring defaults becomes part of the contract and must be reproduced exactly, not approximated:

- Password hashes: Argon2id `$argon2id$v=19$` encoding with Spring Security 5.8 defaults (salt 16 B, hash 32 B, m=16384 KiB, t=2, p=1).
- Access tokens: ES256, claims `sub`/`iat`/`exp`(10 min)/`jti` only, `kid` = RFC 7638 JWK thumbprint when keys are supplied as PEM; JWKS at `/.well-known/jwks.json`.
- Refresh tokens, email-verification tokens and idempotency keys: identical hashing and storage, cookie `refresh_token` with the attributes from ADR 0004.
- Errors: RFC 9457 `application/problem+json` with `type` `https://ledgerly.dev/problems/<slug>`, extensions `code`, `traceId`, `errors[{field,message}]`, including the `REQUEST_REJECTED` cases Spring produced implicitly.
- Paging in Spring Data `PagedModel` shape; audit cursors as unpadded Base64url JSON `{"t","id"}` with the same key order; ETags `"<version>"` accepting `W/"n"`.
- Receipt storage keys in S3 use the same layout.

## Accepted exceptions

### In-flight events

The first deliberate exception is the event outbox. Java stores unfinished events in `event_publication` as Jackson-serialised Java classes, which Rust does not replay. Rust uses the same table with its own event-type names, and each implementation only finishes events it wrote itself. Both may serve HTTP against the same database at the same time, but switching over completely requires the outbox to be drained first; the deployment runbook has that step.

### Idempotency request hashes

Java stores the request hash for `Idempotency-Key` as the SHA-256 of Java's `toString()` of an internal record. Rust hashes its own canonical form of the request instead of imitating Java's debug output. Keys still expire after 24 hours. Consequence: for up to 24 hours after a switch, a retried create whose key the *other* implementation stored is answered with 422 (key reused with a different body) instead of a replay. No expense is ever created twice.
