# The multi-stage build of ADR 0012. Every image is pinned by digest; Renovate proposes updates.
# Hardening is finished in M11 (SBOM, Trivy, non-root check in CI).

# --- chef: the pinned toolchain plus the two cargo sub-commands the build needs ---------------------------------------
FROM rust:1.99.1-bookworm@sha256:e6f2cff35d0f309acf8433b4f05878ed77092602c5065cc81801177da0bb3a9c AS chef
# cargo-chef builds the dependencies in their own layer; cargo-auditable records them in the binary, so a scanner
# can tell what an image contains without the lock file.
RUN cargo install --locked cargo-chef@0.1.78 cargo-auditable@0.7.7
WORKDIR /build

# --- planner: reduce the workspace to the list of its dependencies ----------------------------------------------------
FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

# --- builder: dependencies first (cached until Cargo.lock changes), then our code -------------------------------------
FROM chef AS builder
COPY --from=planner /build/recipe.json recipe.json
RUN cargo chef cook --release --locked --package ledgerly-app --bin ledgerly --recipe-path recipe.json
COPY . .
# Reported by /actuator/info. Without it the member is left out.
ARG LEDGERLY_BUILD_TIME
RUN cargo auditable build --release --locked --package ledgerly-app --bin ledgerly

# --- runtime: glibc, CA certificates, a non-root user (65532), and nothing else: no shell, no package manager --------
FROM gcr.io/distroless/cc-debian12:nonroot@sha256:9dac0a79194e45a7da0158a9c6da57b217585af0786db3845d1f0ec1a0dd182f
COPY --from=builder /build/target/release/ledgerly /usr/local/bin/ledgerly
USER 65532:65532
EXPOSE 8080 8081
ENTRYPOINT ["/usr/local/bin/ledgerly"]
