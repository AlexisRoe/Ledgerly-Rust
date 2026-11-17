# Hardening the local setup

What protects a developer machine and the build from poisoned dependencies, leaked secrets and an exposed dev stack,
and what is still up to you. Every item below was checked; how is in the last column.

## Dev stack (`compose.yaml`)

| Risk | What is done | Checked by |
| --- | --- | --- |
| Database (password `ledgerly`), object store (no auth), ClamAV and Grafana reachable by anyone on the same Wi-Fi | Every port is published on `127.0.0.1` only | `docker compose ps` shows `127.0.0.1:…`; from the machine's LAN address `:8080` gets no answer and `:5432` is closed |
| A tag such as `latest` silently changes content | All five images are pinned by digest (`image:tag@sha256:…`); `docker:pinDigests` in Renovate keeps them current | `docker compose config` |
| The app container can write anywhere and escalate | `read_only`, `tmpfs /tmp`, `cap_drop: ALL`, `no-new-privileges`; the image already runs as non-root | The Bruno run (47 requests, including a receipt upload) passes against that container |

Still true: the services use development passwords (Grafana `admin/admin`, Keycloak `admin/admin`, demo users). That is
why nothing is reachable from outside the machine. Never point `compose.yaml` at a shared server.

## Java build

| Risk | What is done | Checked by |
| --- | --- | --- |
| A tampered Maven download | `distributionSha256Sum` in `.mvn/wrapper/maven-wrapper.properties` | Fresh install with the right sum works; with a wrong sum the wrapper stops: "your Maven distribution might be compromised" |
| A corrupted or swapped dependency | `.mvn/maven.config` has `--strict-checksums`: a checksum mismatch fails the build instead of warning | Full `verify` passes with it on |
| Wrong toolchain, a `-SNAPSHOT` dependency, duplicate versions, plain-http repositories | `maven-enforcer-plugin` rules run on every build | Passes today; a `-SNAPSHOT` dependency added on purpose fails the build with a message |
| Known-vulnerable libraries and base images | Trivy scans the built image in CI; Renovate raises security updates at once; a CycloneDX SBOM is kept per build | CI (not run locally: see the learning log) |
| A freshly published, possibly poisoned release | Renovate waits 3 days before proposing any new version (`minimumReleaseAge`); security fixes are exempt | `renovate.json` |

## Tools run through `npx`

Redocly, Prettier and the Bruno CLI are pinned to exact versions (Makefile, CI, pre-commit), not `@latest`, and run
with `NPM_CONFIG_IGNORE_SCRIPTS=true`, so a package cannot run an install script on your machine. Checked: lint,
format check and the full Bruno run work with scripts off. Update them by changing the version in all places (Renovate
does this as one "build tooling" change).

## CI (`.github/workflows/ci.yml`)

Third-party actions are pinned to a commit SHA with the tag as a comment (a tag can be moved; a commit cannot);
checkouts do not leave the token in `.git/config` (`persist-credentials: false`); the token is read-only.
`aquasecurity/trivy-action` is pinned to `v0.36.0` (commit `ed142fd…`). Advisory GHSA-9p44-j4g5-cfx5 (script injection
in the composite action) affects 0.31.0 to 0.34.0 and is patched from 0.34.1. That version pins its own
`setup-trivy` by commit as well. The earlier pin `v0.28.0` failed because the `setup-trivy` tag it used no longer exists.

## Secrets

- `.gitignore` covers `.env` and every `.env.*` except `.env.example`, `*.pem`, `*.key`, `*.p12`, `*.pfx`, `*.jks`,
  `*.keystore`, `prod.env`, `secrets/`.
- gitleaks runs in pre-commit and in CI over the full history. A scan of all 27 commits and the working tree found
  six false positives (fake test passwords, idempotency keys, an empty template line). They are listed by fingerprint in
  `.gitleaksignore` (not by folder, so a real secret in a test file is still caught: checked with a generated token).
  No real secret was found in the history.
- Real values (token signing keys, database and object-store credentials) belong in the environment of the deployment,
  never in a file in this repository; `docs/deployment.md` lists them and the `prod` profile refuses to start without
  them.

## Yours to do

- Run `pre-commit install` once, so the secret scan and format checks run before each commit.
- Install tools through `mise` (`mise.toml`) and not through `curl | sh`; open a project you do not trust only after
  reading its `mise.toml`, `.mvn/` and `.vscode/`.
- In GitHub, require the CI checks and a pull request before merging to `main` (Settings > Branches).
