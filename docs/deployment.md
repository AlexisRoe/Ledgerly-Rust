# Deploying Ledgerly-Rust

Ledgerly-Rust deploys exactly like [Ledgerly-Java](../vendor/ledgerly-java/docs/deployment.md): same environment variables, same ports, same probes, same demo data ([ADR 0013](adr/0013-java-configuration-and-operational-contract.md)). The differences are the image and the separate migration step.

## Build the image

```sh
just image        # -> ledgerly-rust:<version>
```

The multi-stage `Dockerfile` ([ADR 0012](adr/0012-distroless-runtime-image.md)):

1. Dependencies are cached with cargo-chef on a digest-pinned `rust` image.
2. The release binary is built with `cargo auditable`, so its dependency list is embedded in the binary and scanners can read it.
3. The binary is copied into `gcr.io/distroless/cc-debian12:nonroot`, pinned by digest. It runs as uid 65532, with no shell and no package manager.

On every push, CI builds the image, asserts that it runs as non-root, scans it with Trivy (findings rated high or critical that have a fix fail the build), and keeps a CycloneDX SBOM as a build artifact.

## Migrate the database

Ledgerly-Java ran Flyway inside the app at startup. Ledgerly-Rust doesn't: the official Flyway image applies the vendored migrations as a one-shot step before the app starts ([ADR 0006](adr/0006-apply-java-migrations-unchanged-with-flyway.md)).

```sh
docker run --rm \
  -v "$PWD/vendor/ledgerly-java/src/main/resources/db/migration:/flyway/sql:ro" \
  flyway/flyway:<pinned>@sha256:<digest> \
  -url="$SPRING_DATASOURCE_URL" -user="$SPRING_DATASOURCE_USERNAME" -password="$SPRING_DATASOURCE_PASSWORD" migrate
```

Use the same credentials as the app: V8 revokes write rights on the audit trail from `current_user`, and that must be the app's role. At startup, the app checks that `flyway_schema_history` contains exactly the vendored migrations (version and checksum). If anything differs, it refuses to start.

## Run it

Set `SPRING_PROFILES_ACTIVE=prod`. The prod profile has **no defaults**: unless all of the settings below are present, the app refuses to start and names every missing environment variable.

| Setting (environment variable) | What |
| --- | --- |
| `SPRING_DATASOURCE_URL`, `SPRING_DATASOURCE_USERNAME`, `SPRING_DATASOURCE_PASSWORD` | PostgreSQL. The URL may be in JDBC form (`jdbc:postgresql://host:5432/db`) or `postgres://` form |
| `LEDGERLY_AUTH_JWT_PRIVATE_KEY`, `LEDGERLY_AUTH_JWT_PUBLIC_KEY` | ES256 key pair for access tokens: PKCS#8 (private) and X.509 SubjectPublicKeyInfo (public), as PEM with its line breaks or as the bare Base64 body. The `kid` is the public key's RFC 7638 thumbprint. Not needed with the `keycloak` profile. Use the *same* pair as a Java deployment if both serve the same users |
| `LEDGERLY_STORAGE_S3_ENDPOINT`, `_REGION`, `_BUCKET`, `_ACCESS_KEY`, `_SECRET_KEY` | S3-compatible object store (private bucket) |
| `LEDGERLY_RECEIPTS_CLAMAV_HOST`, `_PORT` | clamd |
| `SPRING_SECURITY_OAUTH2_RESOURCESERVER_JWT_ISSUER_URI` | Only with the `keycloak` profile |

Optional:
- `LEDGERLY_SECURITY_ALLOWED_ORIGINS`: browser origins allowed by CORS. Exact origins only; a wildcard or a path stops the app from starting.
- `LEDGERLY_LIMITS_REQUESTS_PER_MINUTE` (600) and `LEDGERLY_LIMITS_AUTH_REQUESTS_PER_MINUTE` (60).
- `SERVER_FORWARD_HEADERS_STRATEGY=native` behind a reverse proxy, so rate limits see the client's address.
- `MANAGEMENT_OPENTELEMETRY_TRACING_EXPORT_OTLP_ENDPOINT`: where traces are sent.

```sh
docker run --rm --read-only --tmpfs /tmp --cap-drop ALL --security-opt no-new-privileges \
  -p 8080:8080 -p 8081:8081 --env-file prod.env ledgerly-rust:<version>
```

Secrets are held in memory in types that redact themselves in logs and debug output and are wiped from memory when dropped. They are never logged, even at debug level.

## Ports and probes

| Port | What | Publish it? |
| --- | --- | --- |
| 8080 | The API | yes, behind TLS |
| 8081 | `/actuator/health` (+ `/liveness`, `/readiness`), `/actuator/info`, `/actuator/prometheus` | no: only to your orchestrator and metrics scraper |

On SIGTERM the app stops accepting requests and lets running ones and an in-progress import chunk finish, for up to 30 seconds. Give the container a termination grace period of at least that.

## Switching between Java and Rust

Both implementations read and write the same database, object store and tokens ([ADR 0005](adr/0005-drop-in-replacement-on-the-same-data.md)). To switch one deployment from Java to Rust, or back:

1. **Use the same JWT key pair** in both. Otherwise every signed-in user is logged out, though their refresh-token family stays valid in the database.
2. **Check that the schema is at the vendored version.** Rust refuses to start otherwise. If Java is newer than the vendored commit, re-vendor first ([ADR 0007](adr/0007-vendor-shared-artifacts-pinned-to-a-java-commit.md)).
3. **Drain the event outbox.** Stop the old implementation and let its unfinished events complete. Neither side replays the other's events. This query must return 0:
   ```sql
   SELECT count(*) FROM event_publication WHERE completion_date IS NULL;
   ```
   If the old one can't be restarted to drain, list the rows and decide on each one by hand. Each row is a receipt scan, an audit entry or a cleanup that hasn't happened yet.
4. **Expect refused retries for 24 hours.** A create retried with an `Idempotency-Key` the old implementation stored gets 422 instead of a replay ([ADR 0005](adr/0005-drop-in-replacement-on-the-same-data.md#idempotency-request-hashes)). Nothing is created twice.
5. **Swap the image.** Keep every environment variable unchanged.
6. **Run the parity suite** against the new deployment, with `just parity https://…` and a test team, or at least the Bruno collection.

Running both side by side against one database is supported for HTTP traffic. Each side finishes only its own events, and each keeps its own in-memory login lock and rate limits.

## Things to know

- The login lock and the request rate limits live in the memory of one instance; two instances each keep their own. Same as Java.
- Logs are ECS JSON on stdout with `traceId`, `userId` and `teamId`. They never contain emails, tokens, cookies, `Authorization` headers or file contents.

## Using Keycloak instead of Ledgerly's own login

Identical to Java ([Java deployment doc, Keycloak section](../vendor/ledgerly-java/docs/deployment.md#using-keycloak-instead-of-ledgerlys-own-login)). In short:

- Start with `SPRING_PROFILES_ACTIVE=prod,keycloak` and set `SPRING_SECURITY_OAUTH2_RESOURCESERVER_JWT_ISSUER_URI`.
- `/api/v1/auth/*` and the JWKS endpoint disappear.
- Tokens must come from that issuer and carry `ledgerly-api` in `aud`.
- On a person's first valid token, their user is created just in time. If an existing account has the same verified email, it is linked instead of duplicated.

## Demo data

With the `local` profile only (never in `test`, never with `prod` or `keycloak`), an empty database is filled at startup with exactly the data Java's [`DevDataSeeder`](https://github.com/AlexisRoe/Ledgerly-Java/blob/e2a13909634808b347d30b3dec4c3ffee6741c02/src/main/java/com/ledgerly/shared/seed/DevDataSeeder.java) creates ([`reference/demo-data.md`](reference/demo-data.md)): the fixed team id, the same users and the same expenses. That way the Bruno collection runs unchanged. Nothing is added a second time on restart, and `just reset && just up` starts from scratch.

| User | Role in team "Acme Travel" (`00000000-0000-7000-8000-00000000a001`) |
| --- | --- |
| `alice@example.com` | OWNER |
| `bob@example.com` | APPROVER |
| `carol@example.com` | MEMBER (has expenses in every status, one of them rejected with a reason) |
| `dave@example.com` | not a member, email not verified, has a pending invitation |

All four have the password `correct-horse-battery`. They are development credentials, published on purpose.

## API documentation

With the `local` profile, http://localhost:8080/docs opens Swagger UI for the vendored `api/openapi.yaml`. Any other profile serves no documentation: `/docs` and `/swagger-ui/**` are refused like every other path without a token.
