# Vendor the shared files, not the Java code

The files that define parity are copied byte-for-byte from Ledgerly-Java at the commit recorded in `vendor/ledgerly-java/SOURCE`. Each keeps its upstream relative path:

- `api/openapi.yaml`, `redocly.yaml` and `bruno/` sit at the repository root, because they are this repository's contract and request collection too.
- Everything else sits under `vendor/ledgerly-java/`: the Flyway migrations, the `application*.yaml` config defaults, `.env.example`, `compose.yaml`, the Keycloak realm, `http/`, `GLOSSARY.md` (the root `GLOSSARY.md` is a symlink to it), and Java's concept, plan, deployment, hardening and learning-log docs and ADRs 0001–0004.

`vendor/ledgerly-java.SHA256SUMS` lists every vendored file. CI fails if any of them differs or if an unlisted file appears under `vendor/ledgerly-java/`. A scheduled job reports when upstream has moved ahead.

The Java source and tests are deliberately **not** copied. This is a Rust repository. Where the contract is silent, the agent reads the Java code on GitHub at the pinned commit, helped by the extracts in `docs/reference/`. We rejected:

- copying the whole Java repository: it worked offline, but blurred what this repository is;
- relying on the extracts alone: an extraction mistake would go unnoticed with no ground truth to check against;
- a git submodule: awkward for agents and Docker build contexts;
- a one-off copy: it would drift silently.

## Consequences

- Vendored files are never edited here. Re-vendoring to a newer SHA is a deliberate, reviewed change that brings the matching Rust work with it.
- Implementing needs read access to GitHub (`gh` or HTTPS) to consult Java code.
- Repository tooling must leave the vendored files alone. gitleaks excludes `vendor/` and `bruno/environments/` (Java's local-only credentials). Prettier and other formatters exclude `api/`, `redocly.yaml`, `bruno/` and `vendor/`.
