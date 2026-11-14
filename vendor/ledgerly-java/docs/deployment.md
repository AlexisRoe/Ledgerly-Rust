# Deploying Ledgerly

## Build the image

```sh
./mvnw -DskipTests spring-boot:build-image      # -> ledgerly:<version>
```

Cloud Native Buildpacks (Paketo) build it: no Dockerfile to maintain. The image runs on a JRE 25, as a
**non-root user** (uid 1002), and carries a class-data-sharing / AOT cache made by a training run at build time. CI
builds it on every push, checks the user, scans it with Trivy (high and critical findings with a fix fail the
build) and keeps the CycloneDX SBOM as a build artifact (`target/classes/META-INF/sbom/application.cdx.json`).

## Run it

Set `SPRING_PROFILES_ACTIVE=prod`. The prod profile has **no defaults**: it refuses to start, naming every missing
setting and its environment variable, unless all of these are set.

| Setting (environment variable) | What |
| --- | --- |
| `SPRING_DATASOURCE_URL`, `SPRING_DATASOURCE_USERNAME`, `SPRING_DATASOURCE_PASSWORD` | PostgreSQL (Flyway migrates it at start-up) |
| `LEDGERLY_AUTH_JWT_PRIVATE_KEY`, `LEDGERLY_AUTH_JWT_PUBLIC_KEY` | ES256 key pair for access tokens (see `.env.example` for the `openssl` commands). Not needed with the `keycloak` profile |
| `LEDGERLY_STORAGE_S3_ENDPOINT`, `_BUCKET`, `_ACCESS_KEY`, `_SECRET_KEY` | S3-compatible object store (private bucket) |
| `LEDGERLY_RECEIPTS_CLAMAV_HOST`, `_PORT` | clamd |
| `SPRING_SECURITY_OAUTH2_RESOURCESERVER_JWT_ISSUER_URI` | only with the `keycloak` profile |

Optional: `LEDGERLY_SECURITY_ALLOWED_ORIGINS` (browser origins allowed by CORS), `LEDGERLY_LIMITS_*` (request limits),
`SERVER_FORWARD_HEADERS_STRATEGY=native` behind a reverse proxy (so rate limits see the client's address).

```sh
docker run --rm -p 8080:8080 -p 8081:8081 --env-file prod.env ledgerly:0.0.1-SNAPSHOT
```

## Ports and probes

| Port | What | Publish it? |
| --- | --- | --- |
| 8080 | The API | yes, behind TLS |
| 8081 | Actuator: `/actuator/health` (+ `/liveness`, `/readiness`), `/actuator/info`, `/actuator/prometheus` | no: only to your orchestrator and metrics scraper |

On SIGTERM the application stops taking requests and lets running ones finish (up to 30 seconds), so give the
container a `terminationGracePeriodSeconds` of at least that.

## Things to know

- The login lock and the request rate limits are kept in the memory of one instance. Two instances each keep their own
  (see the M10 notes in `learning-log.md`).
- Logs are ECS JSON on stdout with `traceId`, `userId` and `teamId`; they hold no emails, tokens or file contents.

## Using Keycloak instead of Ledgerly's own login

Start with the `keycloak` profile (`SPRING_PROFILES_ACTIVE=prod,keycloak`) and set
`SPRING_SECURITY_OAUTH2_RESOURCESERVER_JWT_ISSUER_URI` to the realm URL (for example
`https://id.example.com/realms/ledgerly`). Then:

- Ledgerly stops issuing tokens: `/api/v1/auth/*` (register, login, refresh, logout, verify) and the JWKS endpoint are
  not registered, and the signing keys are no longer required.
- Tokens must come from that issuer **and** carry `ledgerly-api` in `aud`. A token Keycloak issued to another
  application of the same realm is refused with 401.
- The first valid token of a person creates their Ledgerly user (just-in-time); an existing local account with the same
  *verified* email is linked instead of duplicated. Ledgerly's own user id stays the id everything else refers to.
- `compose.yaml` starts a development Keycloak with `keycloak/ledgerly-realm.json` (users `alice@example.com` /
  `alice-password`, `bob@example.com` / `bob-password`; development only).
- The realm needs an audience mapper for `ledgerly-api` (the export has one), and the `basic` client scope so that
  tokens carry `sub`.

## Demo data

With the `local` profile (never in tests, never with `prod`) an empty database is filled at start-up with a small team,
so the API can be tried at once. `make up` or `make run` is enough; nothing is added a second time on restart, and
`make reset` followed by `make up` starts again from scratch.

| User | Role in team "Acme Travel" (`00000000-0000-7000-8000-00000000a001`) |
| --- | --- |
| `alice@example.com` | OWNER |
| `bob@example.com` | APPROVER |
| `carol@example.com` | MEMBER (has expenses in every status, one of them rejected with a reason) |
| `dave@example.com` | not a member, email not verified, has a pending invitation |

The password of all four is `correct-horse-battery`. With the `keycloak` profile no users are created: the identity
provider owns the accounts (see the Keycloak section above).

## API documentation

With the `local` profile (also `make up`), http://localhost:8080/docs opens Swagger UI for `api/openapi.yaml` (it
redirects to `/swagger-ui/index.html`; the raw file is at `/swagger-ui/openapi.yaml`). No token is needed. Any other
profile serves no documentation endpoint: `/docs` and `/swagger-ui/**` are refused like every other path without a
token, so a deployed instance does not publish its API map. The contract itself is `api/openapi.yaml` in the
repository.
