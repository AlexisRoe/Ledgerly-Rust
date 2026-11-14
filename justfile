set shell := ["bash", "-euo", "pipefail", "-c"]
set dotenv-load := true

# List every task.
default:
    @just --list

# Everything CI checks, in order (docs/local-setup.md §4). The last step, the parity suite, needs Docker.
verify: check parity

# Everything except the parity suite: what the CI `verify` job runs. CI runs the parity suite as its own job.
check: vendor-check fmt-check clippy deny test config-check openapi-lint

# Fail if a file vendored from Ledgerly-Java was changed, removed or added (ADR 0007). Works offline.
vendor-check:
    scripts/vendor-check.sh

# Fail if any file is not formatted.
fmt-check:
    cargo fmt --all --check

# Format every crate.
fmt:
    cargo fmt --all

# Lint all targets; warnings are errors.
clippy:
    cargo clippy --workspace --all-targets --locked -- -D warnings

# Advisories, licences, bans and sources of every dependency (deny.toml).
deny:
    cargo deny check

# The parity tests under crates/parity-tests/tests need a running app and have their own task, `just parity`;
# only the inventory check among them runs here. The integration tests' Postgres is removed when the run ends.
# Unit and integration tests with nextest, then doc-tests (nextest does not run those).
test:
    #!/usr/bin/env bash
    set -euo pipefail
    trap 'just test-db-down' EXIT
    cargo nextest run --workspace --locked -E 'not (package(ledgerly-parity-tests) & kind(test) & not binary(=inventory))'
    cargo test --workspace --locked --doc

# Fail if a YAML or JSON file of ours is not formatted (the vendored ones are in .prettierignore).
config-check:
    npx --yes prettier@3.9.10 --check "**/*.{yml,yaml,json}"

# Format our YAML and JSON files.
config-format:
    npx --yes prettier@3.9.10 --write "**/*.{yml,yaml,json}"

# Lint the vendored OpenAPI file with the vendored rules (redocly.yaml), as Ledgerly-Java does.
openapi-lint:
    npx --yes @redocly/cli@2.76.0 lint api/openapi.yaml

# Regenerate the table of Unicode decimal digits Java's BigDecimal accepts (needs network). Must produce no diff.
gen-unicode-digits:
    scripts/gen-unicode-digits.sh
    cargo fmt --all

# Regenerate the currency table from OpenJDK's ISO 4217 data (needs network). Must produce no diff.
gen-currencies:
    scripts/gen-currencies.sh
    cargo fmt --all

# Regenerate sqlx's offline query metadata in .sqlx/ from the Compose database (`just deps` first). Commit the result.
sqlx-prepare:
    DATABASE_URL="postgres://ledgerly:ledgerly@localhost:5432/ledgerly" \
    cargo sqlx prepare --workspace -- --all-targets --all-features

# `just test` does this at the end of every run; after a bare `cargo nextest run` the container removes itself
# 30 s after the last test disconnected.
# Remove the Postgres container the integration tests share.
test-db-down:
    docker ps --all --quiet --filter "label=org.ledgerly.purpose=integration-tests" | xargs -r docker rm --force --volumes >/dev/null

# --- run and stop ---

image_tag := "ledgerly-rust:0.1.0"

# Start only the dependencies (Postgres, object store, ClamAV, Grafana), then build the schema.
deps:
    docker compose up -d postgres seaweedfs clamav lgtm
    just migrate

# Apply Ledgerly-Java's migrations with the Flyway container (ADR 0006). Does nothing if the schema is current.
migrate:
    docker compose run --rm flyway

# Run the application on the host with the `local` profile, against the Compose services on localhost.
# `just deps` first. A value in the environment or in .env wins over the development defaults here.
run:
    SPRING_PROFILES_ACTIVE="${SPRING_PROFILES_ACTIVE:-local}" \
    SPRING_DATASOURCE_URL="${SPRING_DATASOURCE_URL:-jdbc:postgresql://localhost:5432/ledgerly}" \
    SPRING_DATASOURCE_USERNAME="${SPRING_DATASOURCE_USERNAME:-ledgerly}" \
    SPRING_DATASOURCE_PASSWORD="${SPRING_DATASOURCE_PASSWORD:-ledgerly}" \
    cargo run --locked --bin ledgerly

# Build the container image (ADR 0012).
image:
    docker build --build-arg LEDGERLY_BUILD_TIME="$(date -u +%Y-%m-%dT%H:%M:%SZ)" -t {{ image_tag }} .

# Build the image and run everything in Docker; returns when the app is ready (API http://localhost:8080).
up: image
    docker compose --profile app up -d postgres seaweedfs clamav lgtm app
    scripts/wait-until-ready.sh http://127.0.0.1:8081/actuator/health/readiness

# Containers left by tests (integration-test Postgres, parity app containers) are removed completely.
# Stop the dev environment, the app included; its data stays for the next `just up` or `just deps`.
down: remove-test-containers
    docker compose --profile app stop

alias stop := down

# Remove every container of the project and all their data, dev environment included.
reset: remove-test-containers
    docker compose --profile app down --volumes --remove-orphans

# Remove the containers tests start, with their volumes. Normally the tests do this themselves.
[private]
remove-test-containers: test-db-down
    docker ps --all --quiet --filter "name=^ledgerly-parity-" | xargs -r docker rm --force --volumes >/dev/null

# Follow the application's container logs.
logs:
    docker compose --profile app logs -f app

# Show the running containers.
ps:
    docker compose --profile app ps

# --- acceptance ---

# Without an argument it builds the image and starts the app once per run group; with a base URL it runs the
# default group against that deployment, e.g. `just parity http://127.0.0.1:8080` (set PARITY_DATABASE_URL if it
# is not the Compose database). Every container the run starts is removed at the end; ones that ran before stay.
# Run the black-box parity suite and print passed / ignored / failed per milestone (ADR 0011).
parity url="":
    scripts/parity.sh {{ quote(url) }}

# Run the vendored Bruno collection against a running `local` app (`just up` first).
api-test:
    cd bruno && npx --yes @usebruno/cli@4.3.0 run --env local -r
