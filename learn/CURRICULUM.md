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
