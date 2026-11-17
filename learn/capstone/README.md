# Capstone: shortlinks (reference solution)

The finished version of the service built in lessons 123–128 (section 22). Build your own from `cargo new`
**outside** this repository first; come here to compare, not to copy.

```sh
cd learn/capstone
cargo test                      # 15 tests: domain, store, visit counter, HTTP
cargo clippy --all-targets -- -D warnings
PORT=3000 cargo run             # then, in another terminal:
curl -X POST localhost:3000/links -H 'content-type: application/json' \
     -d '{"target":"https://www.rust-lang.org","slug":"rust"}'
curl -i localhost:3000/rust     # 307 redirect
curl localhost:3000/links/rust  # {"slug":"rust","target":"…","visits":1}
```

| File | Lesson | What it shows |
|---|---|---|
| `Cargo.toml`, `clippy.toml`, `rustfmt.toml` | 123 | a project set up with Ledgerly's rules |
| `src/domain.rs` | 124 | newtypes with smart constructors, a domain error |
| `src/store.rs` | 125 | a port trait and an in-memory adapter behind `RwLock` |
| `src/web.rs` | 126 | axum handlers, problem responses, in-process tests |
| `src/visits.rs`, `src/main.rs` | 127 | a batching worker fed by a channel, graceful shutdown |

Like the playground, this is its own workspace (empty `[workspace]` table) and does not affect `just check`.
