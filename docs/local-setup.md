# Local setup

How to get from a fresh Mac to a running Ledgerly-Rust and a green `just verify`. You do not need Java: Flyway and openapi-generator run from their Docker images.

> The `justfile`, `mise.toml`, `compose.yaml` and the Rust workspace are created in milestone M1 (see [`implementation-plan.md`](implementation-plan.md)). Until then this page describes the target.

## 1. Install by hand (once)

| Tool | Why | How |
| --- | --- | --- |
| [OrbStack](https://orbstack.dev) or Docker Desktop | Runs Postgres, SeaweedFS, ClamAV, Keycloak, Grafana, Flyway, openapi-generator, the integration tests' Postgres and the app image | Download from the vendor's site |
| [mise](https://mise.jdx.dev) | Installs and pins every other tool per project | `brew install mise`, then add `eval "$(mise activate zsh)"` to `~/.zshrc` |
| Git | — | Ships with the Xcode command line tools |

Do not install the other tools with `curl | sh` or globally with `cargo install`. Everything else comes from `mise.toml`, so versions are the same on your machine, the agent's and CI.

## 2. Tools mise installs (`mise install`)

| Tool | Used for |
| --- | --- |
| Rust stable, plus `rustfmt` and `clippy` (exact version also in `rust-toolchain.toml`) | Building; `rust-toolchain.toml` makes cargo and rust-analyzer agree |
| `just` | The task runner: `just` lists every task |
| `cargo-nextest` | Faster test runner with per-test isolation |
| `cargo-deny` | Advisories, licences, banned crates, allowed sources |
| `sqlx-cli` | Regenerates the offline query metadata in `.sqlx/` |
| Node (LTS) | Runs the pinned `npx` tools: Redocly 2.76.0, Prettier 3.9.10, Bruno CLI 4.3.0, with `NPM_CONFIG_IGNORE_SCRIPTS=true` |
| `pre-commit`, `gitleaks` | Secret scan and format checks before each commit |

Before running `mise install` in a repository you did not write, read its `mise.toml` and `.mise/` (if any): mise runs what they say.

## 3. First run

```sh
git clone <this repo> && cd Ledgerly-Rust
mise trust && mise install
pre-commit install
just up        # build the image, start the dependencies, run Flyway, start the app with the `local` profile
```

- API: http://localhost:8080 (Swagger UI at `/docs`, `local` profile only)
- Management: http://localhost:8081/actuator/health
- Grafana: http://localhost:3000 (admin/admin)
- Keycloak (only for the `keycloak` profile): http://localhost:8180 (admin/admin)
- Demo users: see [`deployment.md`](deployment.md#demo-data)

**Apple Silicon:** ClamAV publishes no arm64 image, so it runs under emulation. Its first start downloads virus signatures and takes several minutes. Receipts stay `PENDING_SCAN` until it is ready. This is expected.

## 4. Day to day

| Task | Command |
| --- | --- |
| Start only the dependencies and migrate | `just deps` (runs `just migrate` at the end) |
| Run the app natively (fast edit loop) | `just run`. Sets `SPRING_PROFILES_ACTIVE=local` and points at the Compose services on `localhost` |
| Everything CI checks | `just verify` |
| Run the black-box parity suite | `just parity` (against the running app) or `just parity http://host:port` |
| Run the Bruno collection | `just api-test` |
| Refresh `.sqlx/` after changing a query | `just sqlx-prepare` (needs `just deps`) |
| Start everything in Docker, the app included | `just up` |
| Stop everything, the app included, and keep the data | `just down` (or `just stop`) |
| Remove every container and all data | `just reset` |
| Remove a left-over integration-test Postgres | `just test-db-down` (normally not needed) |

`just verify` runs, in order:
1. the vendor drift check;
2. `cargo fmt --check`;
3. `clippy -D warnings`;
4. `cargo deny check`;
5. `cargo nextest run`, with unit tests and integration tests against Postgres in Docker;
6. Prettier and Redocly lint;
7. the parity suite against a freshly started app.

It needs Docker running.

Builds do not need a database: `SQLX_OFFLINE=true` is the default (`.cargo/config.toml`) and the query metadata in `.sqlx/` is committed.

Integration tests get a database each from `ledgerly_shared::database::TestDatabase`. They share one Postgres container per run (`ledgerly-test-postgres-<digest>`, port on `127.0.0.1` only, data on a tmpfs). Flyway migrates a template database there once; every test gets a copy of it.

No test leaves a container behind:
- `just test` removes the integration-test Postgres when it ends, however it ends. After a bare `cargo nextest run` the container removes itself 30 s after the last test disconnected.
- `just parity` removes every container it created, also on failure or Ctrl-C, volumes included. Containers that existed before (a dev environment from `just up` or `just deps`) are left as they were, running or stopped.

## 5. Hardening checklist (yours to do)

These protect your machine and the repository. The same reasoning as Java's [`local-hardening.md`](../vendor/ledgerly-java/docs/local-hardening.md) applies.

- [ ] Run `pre-commit install` once. Without it the secret scan only happens in CI, after the secret has left your machine.
- [ ] Keep every Compose port on `127.0.0.1`. The services use development passwords (`ledgerly`, `admin/admin`, demo users) and SeaweedFS has no auth at all. Never point `compose.yaml` at a shared server.
- [ ] Never put real values in `.env`. Real keys and credentials belong in the deployment's environment ([`deployment.md`](deployment.md)).
- [ ] Optional, on GitHub: require the CI checks before merging into `main` (Settings › Branches).

## 6. Troubleshooting

| Symptom | Likely cause | First thing to check |
| --- | --- | --- |
| `error: no rows returned` or a sqlx compile error about missing query data | `.sqlx/` is stale after a query change | `just deps && just sqlx-prepare` |
| App exits at startup with "The database schema does not match the migrations this build was made for" | Flyway did not run, or ran a different set of migrations; the message names each version | `just migrate`; `docker compose logs flyway` |
| Receipts never leave `PENDING_SCAN` | ClamAV still downloading signatures (several minutes on first start, slower under emulation) | `docker compose logs clamav` |
| Integration tests fail with "`docker …` failed" | Docker not running, or the `docker` CLI not on `PATH` | `docker info`; `just test-db-down` and run again |
