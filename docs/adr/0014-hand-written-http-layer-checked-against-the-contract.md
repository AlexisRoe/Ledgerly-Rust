# Hand-written HTTP layer, checked against the contract by tests

Supersedes [ADR 0009](0009-server-traits-generated-from-openapi.md).

Routing, request parsing, responses and handlers are written by hand in each module's `web/` layer, with our own serde types. Nothing is generated from `api/openapi.yaml`. The contract is enforced by tests instead of the compiler:

- The router is built from a route table that a test compares with the spec's paths and methods, in both directions (plan M3.6, OWASP API9).
- The parity suite validates real responses against the spec ([ADR 0011](0011-black-box-parity-suite.md)).
- In-process tests cover every documented status code of every endpoint.

## Context

Plan M3.1 evaluated openapi-generator 7.27.0's `rust-axum` output (learning log, M3). Its router cannot be built: it registers paths such as `/teams/{teamId}/expenses/{expenseId}:approve`, and axum 0.8.9, the newest release, pins matchit 0.8.4, which refuses a parameter followed by text in the same path segment. `Router::route` panics at startup for nine operations. Beyond that, the generated handlers answer bad input themselves, before any code of ours runs, with plain-text bodies and statuses that differ from Java's (400 instead of 428 for a missing `If-Match`, 422 instead of 400 for unknown JSON properties), stop at the first missing field where Java lists all of them, authenticate after reading the body, and serve the JWKS document under the wrong path. Working around all of that would have meant patching generated code after every regeneration.

The reviewer chose a fully hand-written layer over generating only the models (decided after the M3.1 evaluation).

## Consequences

- The paths stay exactly Java's. The route table matches `{expenseId}:approve` itself, as Spring's path patterns do: a literal suffix after a parameter, the most specific pattern wins, 404 when no pattern matches, 405 with `Allow` when only the method is wrong.
- Request types are ours, so the order and wording of Java's rejections can be reproduced: content type (415), then headers and path, then body syntax and unknown fields (400 `MALFORMED_REQUEST`), then every Bean Validation rule at once (400 `VALIDATION_FAILED`), then the domain.
- A wrong response shape no longer fails to compile. It fails the parity suite's schema check and the in-process tests instead, one step later in the loop.
- `crates/api` is removed. `just generate` and its drift check are not added.
