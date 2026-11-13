set shell := ["bash", "-euo", "pipefail", "-c"]
set dotenv-load := true

# List every task.
default:
    @just --list

# Everything CI checks, in order. Grows with the milestones (docs/local-setup.md §4).
verify: vendor-check fmt-check clippy deny test config-check openapi-lint

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

# Unit and integration tests with nextest, then doc-tests (nextest does not run those).
test:
    cargo nextest run --workspace --locked
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

# Regenerate the currency table from OpenJDK's ISO 4217 data (needs network). Must produce no diff.
gen-currencies:
    scripts/gen-currencies.sh
    cargo fmt --all

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

# Stop everything and keep the data.
down:
    docker compose --profile app down

# Stop everything and delete the data.
reset:
    docker compose --profile app down -v

# Follow the application's container logs.
logs:
    docker compose --profile app logs -f app

# Show the running containers.
ps:
    docker compose --profile app ps
