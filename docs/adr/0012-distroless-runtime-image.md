# Distroless runtime image

Ledgerly-Java's image comes from Paketo buildpacks, so there is no Dockerfile to port. Ledgerly-Rust builds with a multi-stage Dockerfile: dependencies cached with cargo-chef on a digest-pinned `rust` image, the binary built with `cargo auditable`, and the result copied into `gcr.io/distroless/cc-debian12:nonroot`, pinned by digest. It runs as non-root with a read-only root filesystem, `cap_drop: ALL` and `no-new-privileges`.

**Amended after M11.1:** the runtime image is `gcr.io/distroless/base-nossl-debian12:nonroot` instead of `cc-debian12`, with `libgcc_s.so.1` (and its package record) copied from the builder, which is the same Debian release. The binary links only libc, libm and libgcc_s; it uses rustls, never OpenSSL. `cc` added OpenSSL, libstdc++ and libgomp, which put the image just over 50 MB and brought Trivy findings for a library the binary never loads (CVE-2026-84782 in libssl3 at the time). The image is now about 42 MB.

## Considered options

- `scratch` with a static musl binary: smaller, but musl's allocator is slower and its TLS and DNS edge cases are harder to debug.
- A slim Debian image: easier to debug thanks to its shell, but it adds a shell and a package manager as attack surface.

## Consequences

The image has no shell, so it can't be debugged by exec'ing into it. Debugging uses logs and traces, or an ephemeral debug container attached to it.
