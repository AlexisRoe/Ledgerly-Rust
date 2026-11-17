# Curriculum draft: Rust for TypeScript developers, taught on Ledgerly-Rust

Each lesson is one short HTML page (10–15 minutes). It has:
1. A **warm-up** of 2–3 recall questions from earlier lessons (spaced retrieval).
2. The **TypeScript → Rust comparison**: what you already know, what changes, and where the analogy breaks.
3. **Real code from Ledgerly**: a file and line you open in VS Code.
4. **Practice** with instant feedback: a quiz, "does this compile?", or "predict the output".
5. Often a **hands-on exercise** in `learn/playground/`, a small standalone cargo project with failing tests you make pass (similar to rustlings).
6. A primary source to read, plus a reminder to ask me about anything unclear.

Each section ends with a **cheat sheet** in `learn/reference/` (a TS ↔ Rust table, printable) and a mixed review lesson.

---

## 1 · Setting up and the toolbox
| # | Lesson | You'll be able to… | Ledgerly anchor |
|---|---|---|---|
| 1 | Rust vs TypeScript: the big picture | say what "compiled, no GC, no runtime types" changes compared with Node + tsc | `crates/app/src/main.rs` |
| 2 | rustup and toolchains | read `rust-toolchain.toml` and `mise.toml` the way you'd read `.nvmrc` / `engines` | `rust-toolchain.toml`, `mise.toml` |
| 3 | Cargo is npm + tsc + jest | run `cargo new / build / run / check / test`; read `Cargo.toml` vs `package.json`, `Cargo.lock` vs `package-lock.json` | `crates/expenses/Cargo.toml` |
| 4 | Workspaces | read the root `Cargo.toml`: members, `workspace.dependencies`, `[lints]`, features, profiles | `Cargo.toml` |
| 5 | Quality tools | map rustfmt→Prettier, clippy→ESLint, rust-analyzer→TS server, cargo-deny→npm audit + license checks, nextest | `rustfmt.toml`, `clippy.toml`, `deny.toml` |
| 6 | `just` and the repo's workflow | read the `justfile` the way you'd read npm scripts; know what `just check` runs and why | `justfile` |
| 7 | ★ Review + cheat sheet: commands & config files | | |

## 2 · Basic syntax and code organisation
| # | Lesson | You'll be able to… | Ledgerly anchor |
|---|---|---|---|
| 8 | Hello, Rust: `fn main`, `let`, `mut`, `println!` | write and run a small program; know that `!` means "macro" | playground |
| 9 | Statements vs expressions, and the missing semicolon | read why the last line of a function returns without `return` | `Action::as_str` |
| 10 | Comments and docs | tell `//`, `///` and `//!` apart (vs JSDoc/TSDoc); know that doc examples run as tests | `crates/shared/src/id.rs` |
| 11 | Files become modules | `mod x;` → `x.rs` or `x/mod.rs`; `lib.rs`/`main.rs` as the entry (vs `index.ts`); the module tree is declared, not found by scanning | `crates/expenses/src/lib.rs` |
| 12 | Imports with `use` | `crate::`, `super::`, `self::`, braces, `as`; external crates (vs `import … from`) | `crates/expenses/src/web/mod.rs` |
| 13 | Visibility and re-exports | private by default; `pub`, `pub(crate)`; `pub use` (vs `export … from`); facades (ADR 0010) | `crates/expenses/src/lib.rs` |
| 14 | Folder structure of a crate and of this repo | `src/`, `tests/`, `build.rs`, `benches`; Ledgerly's `domain / application / persistence / web` layers | `crates/*` |
| 15 | ★ Review + cheat sheet: syntax & modules | | |

## 3 · Data types
| # | Lesson | You'll be able to… | Ledgerly anchor |
|---|---|---|---|
| 16 | Numbers: one `number` becomes many | pick between `i32`, `u8`, `u32`, `i64`, `usize`, `f64`; overflow; `as` casts; why money isn't `f64` | `ReceiptSummary`, `Money` |
| 17 | `bool`, `char` and type inference | read inferred vs annotated types; `let x: u32 = …`; turbofish `::<>` | playground |
| 18 | Strings: `String` vs `&str` | know when you own the text and when you borrow it; `&'static str`; `format!` (vs template literals) | `Action::as_str`, `text.rs` |
| 19 | Tuples, arrays, slices, `Vec` | tell `[T; N]`, `&[T]` and `Vec<T>` apart (vs `T[]` and tuples) | `Action::ALL`, `rejections()` |
| 20 | Structs: like interfaces, plus data | define, build and update structs; field init shorthand; tuple structs | `Criteria`, `ExpensesApi` |
| 21 | `impl` blocks: methods and associated functions | `self` / `&self` / `&mut self`; `Self`; `new` by convention (vs `class`) | `ExpensesApi::new` |
| 22 | Enums I: plain variants | vs string literal unions | `Action`, `ExpenseStatus` |
| 23 | Enums II: variants with data | vs discriminated unions (`{ kind: "x", … }`) | `ExpenseError` |
| 24 | `Option<T>`: no `null`, no `undefined` | read and handle `Option` (vs `T \| undefined`, `?.`, `??`) | `Expense::note`, `Criteria` |
| 25 | `Result<T, E>`: errors as values | vs `throw`/`try`; preview of `?` | `persistence::find` |
| 26 | Newtypes: branded types done right | `struct TeamId(Uuid)` vs `string & { __brand }`; "parse, don't validate" | `id.rs`, `Merchant` |
| 27 | Generic types you'll see everywhere | read `Vec<T>`, `HashMap<K,V>`, `Option<Result<…>>`, type aliases | across the code |
| 28 | ★ Review + cheat sheet: data types | | |

## 4 · Control flow
| # | Lesson | You'll be able to… | Ledgerly anchor |
|---|---|---|---|
| 29 | `if` is an expression | replace ternaries; no truthiness (only `bool`) | `ExpensesApi::routes` |
| 30 | Loops: `loop`, `while`, `for` | ranges, `break` with a value, labels; `for … in` vs `for…of` | playground |
| 31 | `match`: a `switch` that checks everything | exhaustiveness (vs the `never` trick), `\|`, ranges, guards, `_` | `Action::target_from` |
| 32 | Patterns and destructuring | destructure structs, enums and tuples in `let`, `match` and parameters | `parse_canonical` |
| 33 | `if let`, `let … else`, `while let`, `matches!` | handle one case without a full `match` | across the code |
| 34 | Early exit: `return` and `?` | read `?` chains (vs `await` + try/catch) | `persistence::find` |
| 35 | ★ Review + cheat sheet: control flow | | |

## 5 · Ownership and borrowing (the part with no TypeScript equivalent)
| # | Lesson | You'll be able to… | Ledgerly anchor |
|---|---|---|---|
| 36 | Why ownership: memory without a GC | explain stack vs heap and who frees what | playground |
| 37 | Move semantics | predict "value used after move" (vs JS sharing references) | playground |
| 38 | `Copy` and `Clone` | know why ids are `Copy` and `pool.clone()` is cheap | `ExpensesApi::new` |
| 39 | Borrowing: `&` and `&mut` | apply "many readers or one writer" (vs `readonly`) | `Expense` methods |
| 40 | Reading the borrow checker's errors | fix E0382, E0499, E0502 from the message | playground |
| 41 | Lifetimes, enough to read them | read `'a`, `'static` and elision; no need to write complex ones | `&'static str` in `ExpenseError` |
| 42 | Shared ownership: `Box`, `Rc`, `Arc` | read `Arc<dyn Clock>` and why it's everywhere | `lib.rs::routes` |
| 43 | ★ Review + cheat sheet: ownership rules | | |

## 6 · Functions, closures and iterators
| # | Lesson | You'll be able to… | Ledgerly anchor |
|---|---|---|---|
| 44 | Function signatures in depth | read parameters by value / `&` / `&mut`, `impl Trait`, `#[must_use]`, `const fn` | `Action` |
| 45 | Closures vs arrow functions | `\|x\| x + 1`, `move`, capturing | `ExpensesApi::routes` |
| 46 | Iterators vs array methods | `iter().map().filter().collect()`; laziness; `enumerate`, `all`, `any` | `parse_canonical` |
| 47 | Collecting into `Result` and `Option` | read `collect::<Result<Vec<_>,_>>()`, `transpose`, `map_err` | `persistence` |
| 48 | ★ Review + cheat sheet | | |

## 7 · Traits and generics
| # | Lesson | You'll be able to… | Ledgerly anchor |
|---|---|---|---|
| 49 | Traits are interfaces, but nominal | `trait` + `impl Trait for Type` (vs structural typing) | `Clock` |
| 50 | Derive and the standard traits | `Debug, Clone, Copy, PartialEq, Eq, Hash, Default` | `#[derive]` everywhere |
| 51 | Conversions: `From`/`Into`, `TryFrom`, `FromStr`, `Display` | read `.into()`, `.parse()`, `{}` formatting | `id.rs` |
| 52 | Generics with trait bounds | `fn f<T: Trait>`, `where` (vs `<T extends …>`) | |
| 53 | Trait objects: `dyn Trait` | static vs dynamic dispatch; `Send + Sync` | `Arc<dyn Publisher>` |
| 54 | ★ Review + cheat sheet | | |

## 8 · Error handling the Ledgerly way
| # | Lesson | You'll be able to… | Ledgerly anchor |
|---|---|---|---|
| 55 | `?` and `From`: errors that convert themselves | follow an error up the call stack | `#[from]` in `ExpenseError` |
| 56 | Custom errors with `thiserror` | read `#[error("…")]`, `#[error(transparent)]` | `domain/error.rs` |
| 57 | Panics vs errors, and why `unwrap` is banned here | clippy denies; when tests may unwrap | `Cargo.toml` lints |
| 58 | From domain error to HTTP problem | follow `ExpenseError` → `problem+json` response | `web/problem.rs` |
| 59 | ★ Review | | |

## 9 · Macros and attributes (reading, not writing)
| # | Lesson | You'll be able to… | Ledgerly anchor |
|---|---|---|---|
| 60 | Attributes: `#[…]` and `#![…]` | `derive`, `cfg(test)`, `must_use`, `expect`, `forbid` (vs decorators) | across the code |
| 61 | Declarative macros you'll meet | `vec!`, `format!`, `matches!`, `tracing::info!`, reading `macro_rules! typed_id` | `id.rs` |

## 10 · Async Rust
| # | Lesson | You'll be able to… | Ledgerly anchor |
|---|---|---|---|
| 62 | Futures vs Promises | `async fn`/`.await`; futures are lazy; you need a runtime (vs the Node event loop) | `main.rs` |
| 63 | Tokio: the runtime | `block_on`, `spawn`, `join!`, `select!`, `#[tokio::test]` | `main.rs`, `server.rs` |
| 64 | Sharing state across tasks | `Arc`, `Send`/`Sync`, why `Clone` on handler state | `ExpensesApi` |

## 11 · Testing
| # | Lesson | You'll be able to… | Ledgerly anchor |
|---|---|---|---|
| 65 | Unit tests next to the code | `#[cfg(test)] mod tests`, `#[test]`, `assert_eq!` (vs Jest `describe/it`) | `domain/expense.rs` |
| 66 | Integration, doc and property tests | `tests/`, `compile_fail` doc tests, `proptest` (vs fast-check) | `money/mod.rs`, `id.rs` |
| 67 | Ledgerly's test layers | unit vs app tests vs the black-box parity suite; Docker; `just test` vs `just parity` | `crates/parity-tests` |

## 12 · The libraries Ledgerly is built on
| # | Lesson | You'll be able to… | Ledgerly anchor |
|---|---|---|---|
| 68 | serde: JSON in and out | `#[derive(Serialize, Deserialize)]`, `rename_all` (vs zod / `JSON.parse`) | `web/dto.rs` |
| 69 | sqlx: SQL checked at compile time | `query_as!`, `.sqlx/` offline data, transactions (vs Prisma / Kysely) | `persistence/mod.rs` |
| 70 | axum and Ledgerly's `Routes` | routes, handlers, state, responses (vs Express / Fastify) | `web/mod.rs`, `shared/web/routing.rs` |
| 71 | tracing, metrics and secrecy | structured logs, spans, `SecretString` | `logging.rs`, `config.rs` |

## 13 · Reading Ledgerly end to end
| # | Lesson | You'll be able to… | Ledgerly anchor |
|---|---|---|---|
| 72 | The crate map | name what each of the 9 crates does and which may depend on which | `crates/app/tests/architecture.rs` |
| 73 | Following one request: submit an expense | route → handler → application → domain → persistence → event | `expenses` crate |
| 74 | Startup: from `main` to a listening server | config → logging → database → `App::bind` | `crates/app` |

## 14 · Making changes (capstone)
| # | Lesson | You'll be able to… | Ledgerly anchor |
|---|---|---|---|
| 75 | The change loop | `cargo check` → read the error → fix → `clippy` → `fmt` → `test`; compiler errors compared with `tsc`'s | |
| 76 | Exercise: change a domain rule and its test | edit, follow the compiler, go green | `domain/` |
| 77 | Exercise: add an enum variant and let `match` guide you | see exhaustiveness pay off | `Action` / `Forbidden` |
| 78 | Exercise: add a unit test, then a small helper function | | |
| 79 | Where to go next | | |

Capstone exercises run on a throwaway branch and are reverted afterwards: Ledgerly's schema and contract come from Ledgerly-Java, so real behaviour changes stay out.

---

**Reference documents** (`learn/reference/`): TS↔Rust syntax sheet, tool and command sheet, data types sheet, control flow sheet, ownership rules card, traits table, error-handling flow, a glossary, and a Ledgerly map (crate → layer → file).

---

# Part II · Rust in depth (added 2026-11-17)

Part I teaches the Rust that Ledgerly uses, so you can read it and change it. Part II is for an experienced backend
developer who wants to understand the language itself: what's underneath, how to design Rust code, and how
concurrency works when there is more than one thread. The format is the same: TS comparison, real Ledgerly code
where it exists, quizzes, a playground exercise for most lessons. It ends with a capstone that builds a small
service from an empty `cargo new`.

## 15 · Ownership and lifetimes in depth
| # | Lesson | You'll be able to… | Anchor |
|---|---|---|---|
| 80 | Writing lifetime annotations | apply the three elision rules; annotate a function the compiler can't infer | `java_lines` |
| 81 | Structs that borrow | design `struct Parser<'a>`; know when a borrowing struct is worth it | `receipts::Modules<'a>` |
| 82 | `'static` and `T: 'static` | tell "lives forever" from "owns all its data"; read why `tokio::spawn` needs `'static` | `ensure_bucket` |
| 83 | Borrowing in practice | split borrows of fields; the entry API; "get, then insert" patterns that compile | playground |
| 84 | Interior mutability | choose `Cell`, `RefCell`, `Mutex`, `OnceLock`/`LazyLock` | `FixedClock`, `match_nothing` |
| 85 | `Cow`: borrow when you can | return borrowed or owned data from one function | `typed_id` deserialiser |
| 86 | Drop, RAII and `mem::take` | write `impl Drop` guards; move out of a `&mut` with `take`/`replace` | `Observation`, `java_lines` |
| 87 | ★ Review + cheat sheet | | |

## 16 · Traits in depth
| # | Lesson | You'll be able to… | Anchor |
|---|---|---|---|
| 88 | Associated types vs generic parameters | know why `Iterator` has `Item` but `From` has `<T>` | `impl Iterator<Item = &str>` |
| 89 | Implementing `Iterator` yourself | write `next`, get all adapters free; `IntoIterator`; `iter::from_fn` | `java_lines` |
| 90 | Default methods, blanket impls, extension traits | add behaviour to every type that has a trait | `Clock::today` |
| 91 | Coherence: the orphan rule | know which impls you may write; the newtype workaround | `typed_id` |
| 92 | `dyn` compatibility and async in traits | know which traits can be `dyn`; read `BoxFuture` ports | `MembershipLookup` |
| 93 | `AsRef`, `Borrow`, `Deref` and flexible parameters | accept `impl AsRef<str>` / `impl Into<String>`; avoid `Deref` abuse | `invalid(…, impl Into<String>)` |
| 94 | Comparison and operator traits | the contracts of `Eq`, `Ord`, `Hash`; overload `+`; when *not* to | `Money::cmp` |
| 95 | Typestate: states as types | make invalid transitions fail to compile; `PhantomData` | vs `ExpenseStatus` |
| 96 | ★ Review + cheat sheet | | |

## 17 · Concurrency and async internals
| # | Lesson | You'll be able to… | Anchor |
|---|---|---|---|
| 97 | Threads and scoped threads | spawn and join OS threads; borrow from the stack with `thread::scope` | clock test |
| 98 | `Send` and `Sync`, really | explain what each means and read the errors when they're missing | `Clock: Send + Sync` |
| 99 | Shared mutable state | `Arc<Mutex<_>>`, `RwLock`, atomics, poisoning, std vs tokio `Mutex` | `FixedClock` |
| 100 | Channels and the actor pattern | `mpsc`, `oneshot`, `watch`, `broadcast`; own state in one task | `Shutdown` |
| 101 | Inside a future: `poll` and `Waker` | write a `Future` by hand and a tiny `block_on` executor | playground |
| 102 | `Pin`, just enough | why self-referential futures need pinning; read `Pin<Box<dyn Future>>` | `BoxFuture` |
| 103 | Cancellation and structured concurrency | cancel-safety, `select!` in loops, `JoinSet`, aborting | `server.rs` |
| 104 | Blocking inside async | `spawn_blocking` for CPU work; why it hurts more than in Node | `hash_password` |
| 105 | ★ Review + cheat sheet | | |

## 18 · Memory and performance
| # | Lesson | You'll be able to… | Anchor |
|---|---|---|---|
| 106 | Stack, heap and layout | predict `size_of`; niches make `Option<Box<T>>` free | playground |
| 107 | Where allocations hide | `with_capacity`, `&str` params, avoiding clones and temporary `Vec`s | `hex` in `upload.rs` |
| 108 | Generics vs `dyn`: what abstractions cost | monomorphisation, inlining, code size and compile times | `Arc<dyn Clock>` |
| 109 | Measuring: release builds, benchmarks, profiles | `--release`, `black_box`, criterion, flamegraphs | playground `bench` |
| 110 | ★ Review + cheat sheet | | |

## 19 · Designing Rust code
| # | Lesson | You'll be able to… | Anchor |
|---|---|---|---|
| 111 | Error design: libraries vs applications | `thiserror` enums vs `anyhow` with context | `ExpenseError` vs `main` |
| 112 | Parse, don't validate, in depth | private fields, smart constructors, `serde(try_from)` | `text.rs` |
| 113 | Constructors, `Default` and builders | pick between `new`, `Default`, struct update and a builder | `Limits`, runtime `Builder` |
| 114 | APIs others enjoy | naming (`as_`/`to_`/`into_`), `#[must_use]`, `#[non_exhaustive]`, the API guidelines | across the code |
| 115 | Module and crate boundaries | facades, features, semver; when to split a crate | ADR 0010 |
| 116 | Documentation as code | rustdoc, doc tests, intra-doc links, `# Errors` sections | `Money` |
| 117 | ★ Review + cheat sheet | | |

## 20 · Writing macros
| # | Lesson | You'll be able to… | Anchor |
|---|---|---|---|
| 118 | `macro_rules!` from scratch | matchers and fragment types; write a small macro | `display_as_text!` |
| 119 | Repetition, internal rules and hygiene | `$(…),*`, `@internal` arms, why macros don't capture names | `required_in_prod!`, `typed_id!` |
| 120 | Procedural macros, and when not to write one | how derives work, `cargo expand`, functions first | `#[derive]`, `#[tokio::main]` |

## 21 · `unsafe`, briefly
| # | Lesson | You'll be able to… | Anchor |
|---|---|---|---|
| 121 | What `unsafe` allows | the five superpowers; why Ledgerly forbids it | `#![forbid(unsafe_code)]` |
| 122 | Reading `unsafe` in dependencies | `// SAFETY:` comments, Miri, how to judge a crate | `deny.toml` |

## 22 · Capstone: a service from scratch
A small link-shortener service, built by you from `cargo new`. A complete reference solution lives in
`learn/capstone/`; each lesson shows the decisions, gives tests to start from, and links the solution's code.

| # | Lesson | You'll be able to… |
|---|---|---|
| 123 | Plan and set up | create the crate, lints, dependencies; turn requirements into types |
| 124 | The domain, types first | newtypes with smart constructors, a domain error, unit tests |
| 125 | Storage behind a trait | a `LinkStore` port, an in-memory adapter with `RwLock`, `Arc<dyn …>` |
| 126 | HTTP with axum | handlers, state, errors → responses, in-process tests with `oneshot` |
| 127 | Background work and shutdown | count visits through a channel and a worker task; graceful shutdown |
| 128 | Review your own code | clippy pedantic, docs, the API checklist; where to go next |

New cheat sheets: ownership in depth, traits in depth, concurrency, performance, design, macros and `unsafe`,
capstone checklist.
