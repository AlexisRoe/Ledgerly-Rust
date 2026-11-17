# Rust (for TypeScript developers) Resources

All links were checked on 2026-11-17.

## Knowledge

### Core texts
- [Book: _The Rust Programming Language_ ("the Book"), Klabnik, Nichols & Krycek](https://doc.rust-lang.org/book/)
  The official introduction, free and kept current. Use for: almost every language lesson; the primary source for sections 2–8.
- [Rust by Example](https://doc.rust-lang.org/rust-by-example/)
  Short runnable examples per feature. Use for: quick "show me" moments, `let else`, `if let`, conversions, testing.
- [Comprehensive Rust (Google)](https://google.github.io/comprehensive-rust/)
  Google's 4-day course for experienced developers, compact and well ordered. Use for: a second explanation when the Book's doesn't click.
- [The Rust Reference](https://doc.rust-lang.org/reference/)
  The precise language definition. Use for: attributes, modules, visibility, conditional compilation. Dense; for checking, not for learning.
- [Standard library docs](https://doc.rust-lang.org/std/)
  Use for: `Option`, `Result`, `Iterator`, `String`, `Arc`, `From`; every method with an example.
- [rustc error index](https://doc.rust-lang.org/error_codes/)
  Use for: reading an error such as E0382 with a minimal example and its fix.

### Tooling
- [The rustup book](https://rust-lang.github.io/rustup/): toolchains, `rust-toolchain.toml` overrides.
- [The Cargo book](https://doc.rust-lang.org/cargo/): manifest, workspaces, features, profiles, build scripts, `Cargo.lock`.
- [Clippy docs](https://doc.rust-lang.org/clippy/) and the [lint list](https://doc.rust-lang.org/clippy/lints.html): what each lint means.
- [rustfmt](https://rust-lang.github.io/rustfmt/): formatter configuration options.
- [rust-analyzer](https://rust-analyzer.github.io/): the language server behind VS Code's Rust support.
- [cargo-deny book](https://embarkstudios.github.io/cargo-deny/): advisories, licences, bans, sources.
- [cargo-nextest](https://nexte.st/): the test runner `just test` uses.
- [just manual](https://just.systems/man/en/): the command runner behind `justfile`.
- [mise](https://mise.jdx.dev/): pins tool versions (`mise.toml`).
- [The Edition Guide](https://doc.rust-lang.org/edition-guide/): what `edition = "2024"` means.
- [rustdoc: documentation tests](https://doc.rust-lang.org/rustdoc/write-documentation/documentation-tests.html): code examples in doc comments that run as tests.

### Async and the libraries Ledgerly uses
- [Tokio tutorial](https://tokio.rs/tokio/tutorial): runtime, `spawn`, shared state, `select!`. Use for: section 10.
- [Asynchronous Programming in Rust (async book)](https://rust-lang.github.io/async-book/): background on futures.
- [serde.rs](https://serde.rs/): derive, attributes (`rename_all`, `deny_unknown_fields`).
- [sqlx docs](https://docs.rs/sqlx/latest/sqlx/): `query!`, `query_as!`, offline mode.
- [axum docs](https://docs.rs/axum/latest/axum/) and [tower](https://docs.rs/tower/latest/tower/): routing, handlers, middleware.
- [thiserror](https://docs.rs/thiserror/latest/thiserror/): `#[derive(Error)]`, `#[from]`, `#[error(transparent)]`.
- [tracing](https://docs.rs/tracing/latest/tracing/): structured logging and spans.
- [secrecy](https://docs.rs/secrecy/latest/secrecy/): `SecretString`.
- [proptest book](https://proptest-rs.github.io/proptest/): property-based testing (like fast-check).

### Design ideas
- [Alexis King, "Parse, don't validate"](https://lexi-lambda.github.io/blog/2019/11/05/parse-don-t-validate/): why Ledgerly's newtypes (`Merchant`, `TeamId`) validate once in a constructor.
- [Rust API Guidelines: naming](https://rust-lang.github.io/api-guidelines/naming.html): `new`, `as_`, `to_`, `into_`, `try_`.
- [TypeScript handbook: narrowing](https://www.typescriptlang.org/docs/handbook/2/narrowing.html): the TS side of discriminated unions and exhaustiveness checks, for comparison.

### Practice
- [Rustlings](https://rustlings.rust-lang.org/): small exercises with failing tests. Same idea as `learn/playground/`; good for extra reps once you finish a section.

## Wisdom (Communities)

- [Rust Users Forum](https://users.rust-lang.org/)
  The official forum for questions about using Rust. Friendly to beginners, well moderated. Use for: "why doesn't this compile?", design questions.
- [r/learnrust](https://www.reddit.com/r/learnrust/)
  Beginner-focused subreddit. Use for: quick questions and code review of small snippets.
- [This Week in Rust](https://this-week-in-rust.org/)
  Weekly newsletter. Use for: keeping up with the ecosystem once you can read Rust comfortably.

## Gaps
- No single trustworthy "Rust for TypeScript developers" text was found; the comparisons in the lessons are built from the Book plus the TypeScript handbook. Every claim about Rust is backed by a link to an official source.
