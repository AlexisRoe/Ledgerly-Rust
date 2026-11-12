# Instructions for the implementing agent

You implement Ledgerly-Rust by following [`docs/implementation-plan.md`](docs/implementation-plan.md), one step at a time. The design is settled; your job is to carry it out faithfully and leave a trail a reviewer and a later lesson-writing agent can follow.

## Read before the first step of a session

1. [`MISSION.md`](MISSION.md) and [`CONCEPT.md`](CONCEPT.md): goal, architecture, security rules, definition of done.
2. [`GLOSSARY.md`](GLOSSARY.md): the domain language. Use its terms in type, function, module and test names. Avoid every word listed under `_Avoid_`.
3. [`docs/adr/`](docs/adr/): every decision, including Java's 0001–0004, which apply here unchanged.
4. The current milestone in the plan, and the matching milestone in [`vendor/ledgerly-java/docs/learning-log.md`](vendor/ledgerly-java/docs/learning-log.md). It lists traps Java already hit.

## Where behaviour comes from

The specification is Ledgerly-Java at the commit in `vendor/ledgerly-java/SOURCE`. The shared files are vendored in this repository ([ADR 0007](docs/adr/0007-vendor-shared-artifacts-pinned-to-a-java-commit.md)), but the Java source and tests are **not**: read those on GitHub at that exact commit, never on `main`. Use the first source that answers the question:

1. `api/openapi.yaml`: paths, shapes, status codes.
2. `vendor/ledgerly-java/src/main/resources/db/migration/`: the schema. Rust never changes it.
3. [`docs/reference/`](docs/reference/): extracts of the Java code and tests. Fast, but a map, not the authority.
4. The Java tests at [`src/test/java/`](https://github.com/AlexisRoe/Ledgerly-Java/blob/e2a13909634808b347d30b3dec4c3ffee6741c02/src/test/java/com/ledgerly/): what is asserted is required.
5. The Java main code at [`src/main/java/`](https://github.com/AlexisRoe/Ledgerly-Java/blob/e2a13909634808b347d30b3dec4c3ffee6741c02/src/main/java/com/ledgerly/): what it does is required, including Spring defaults that leak into responses, hashes or stored data ([ADR 0005](docs/adr/0005-drop-in-replacement-on-the-same-data.md) lists the known ones).

For 4 and 5, fetch files with `gh api "repos/AlexisRoe/Ledgerly-Java/contents/<path>?ref=<commit>" --jq .content | base64 -d` (or WebFetch on the `raw.githubusercontent.com` URL). If a reference doc turns out to be wrong, fix it in the same commit as the code that relies on it.

If these disagree with each other, or the Java behaviour looks like a bug, **do not choose**. Implement what Java does, add the question to the milestone's learning-log entry under "Open questions", and mention it when you stop.

## Hard rules

- **Never modify the vendored files:** `vendor/`, `api/openapi.yaml`, `redocly.yaml` and `bruno/`. The drift check fails the build if you do. Schema, contract or collection changes happen in Ledgerly-Java first.
- **Never weaken a parity test to make it pass.** If you believe a parity test is wrong, leave it failing, mark it `#[ignore = "reason"]` only with a reason that cites the Java file, and raise it when you stop.
- **No `unsafe`.** Every crate has `#![forbid(unsafe_code)]`.
- **No `unwrap`, `expect`, `panic!`, `todo!` or `unimplemented!` outside tests.** Clippy denies them. Return errors.
- **Secrets stay secret.** Passwords, tokens, keys, cookies and `Authorization` headers are wrapped in `secrecy::SecretString`/`SecretBox` from the moment they are read. Never log them, never `Debug`-print them, never put them in error messages. The log-hygiene tests enforce this.
- **Every query that reads team data takes the team id.** There is no "find by id" without a team (OWASP API1).
- **New dependencies:** prefer crates the plan names. For anything else, say why in the commit message. `cargo deny check` must pass, so no new licences, no git or path sources outside the workspace, no yanked or advised versions. Don't add a crate for something the standard library or an existing dependency does.
- **No network access at runtime** except to the configured Postgres, S3, ClamAV, OTLP endpoint and (with `keycloak`) the issuer.

## How to work a step

1. Pick the first unchecked `- [ ]` step of the current milestone.
2. Write the failing test first. For behaviour, this is often a parity test that is currently ignored.
3. Implement it. Keep each crate's public API to its facade and events ([ADR 0010](docs/adr/0010-one-crate-per-module.md)).
4. Run `just verify`. It must be green, with no new warnings and no new ignored tests without a reason.
5. Tick the step's checkbox in `docs/implementation-plan.md`.
6. Commit everything to the **current branch** in one Conventional Commit (`feat(expenses): …`, `test(parity): …`). One step, one commit.
7. Go on with the next step.

Never push, never rebase or amend published history, never switch branches, never touch `main`.

## At the end of a milestone

1. Check the milestone against the definition of done in `CONCEPT.md`.
2. Write the milestone's entry in [`docs/learning-log.md`](docs/learning-log.md), in the format at the top of that file. Write it for a reader who has the code and the Java, but not this session.
3. Commit it as `docs(learning-log): M<N>`.
4. **Stop.** Report what was done, what parity tests now pass, the open questions, and anything you were unsure about. Wait for the reviewer before starting the next milestone.

Do not write lessons. A separate agent writes them after M11.

## When you are stuck

- If a step is unclear or contradicts an ADR, stop and ask; don't guess.
- If the generated server code (`crates/api`) can't express part of the spec, record the gap in the learning log and stop. Per [ADR 0009](docs/adr/0009-server-traits-generated-from-openapi.md), switching any part to hand-written handlers needs a new ADR, which the reviewer decides.
- If a test needs Docker and Docker isn't available, say so; don't skip the test.
