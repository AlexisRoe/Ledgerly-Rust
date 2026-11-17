# The multi-stage build of ADR 0012. Every image is pinned by digest; Renovate proposes updates.
# CI (the `image` job) checks that it runs as non-root, scans it with Trivy and keeps a CycloneDX SBOM of it.
# No secret is needed to build it, so none can end up in a layer: the one build argument is a timestamp.

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

# The one library the binary needs that the runtime image lacks: libgcc_s, which unwinding uses (`panic` stays
# `unwind`, see Cargo.toml). Taken from the builder, which is the same Debian release as the runtime, with its
# package record, so that Trivy and the SBOM know it is there. The paths depend on the architecture.
RUN set -eu; \
    triplet="$(gcc -dumpmachine)"; \
    mkdir -p "/runtime/usr/lib/$triplet" /runtime/var/lib/dpkg/status.d; \
    cp "$(gcc -print-file-name=libgcc_s.so.1)" "/runtime/usr/lib/$triplet/"; \
    dpkg-query --status libgcc-s1 > /runtime/var/lib/dpkg/status.d/libgcc-s1

# --- runtime: glibc, CA certificates, a non-root user (65532), and nothing else: no shell, no package manager, no
# OpenSSL (the binary uses rustls) and no C++ runtime. -----------------------------------------------------------------
FROM gcr.io/distroless/base-nossl-debian12:nonroot@sha256:be40c00dfabd86576d92666e87e406714d5618342de1a0c213ad232de255172e
COPY --from=builder /runtime/ /
COPY --from=builder /build/target/release/ledgerly /usr/local/bin/ledgerly
USER 65532:65532
EXPOSE 8080 8081
ENTRYPOINT ["/usr/local/bin/ledgerly"]
