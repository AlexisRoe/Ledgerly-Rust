# Mission: Rust backend fundamentals

## Why

Learn backend development in Rust on a system whose behaviour is already pinned down. Ledgerly-Java defines *what* the system does. This repository is only about *how* Rust does it, so every difference between the two comes from the language and its ecosystem and not from a changed requirement.

## Success looks like

- Ledgerly-Rust is a working drop-in replacement: the parity suite passes, and swapping the Java image for the Rust image on an existing database changes nothing for clients.
- For any file here, you can explain what each line does and why Rust needs it that way (ownership, lifetimes, traits, `async`, error handling).
- You can point at the Java file that matches any Rust file, and say what the Rust type system now guarantees that Java checked at runtime or in tests.
- You can add a field or an endpoint end to end (contract upstream, migration upstream, route, handler, domain, SQL, tests) and keep `just verify` green.

## How the work is split

- A coding agent implements the plan milestone by milestone and stops after each one for your review (see [`CLAUDE.md`](CLAUDE.md)).
- After each milestone it writes an entry in [`docs/learning-log.md`](docs/learning-log.md): which Java construct became which Rust construct, and why.
- Lessons are **not** written during implementation. A separate agent writes them from the learning log once M11 is done.

## Out of scope

- Changing what Ledgerly does. Any behaviour change happens in Ledgerly-Java first and is then re-vendored ([ADR 0007](docs/adr/0007-vendor-shared-artifacts-pinned-to-a-java-commit.md)).
- A frontend, real payments, real email delivery (emails stay a logged event, as in Java).
