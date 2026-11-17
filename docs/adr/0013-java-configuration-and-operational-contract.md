# Same configuration and operational surface as Java

Ledgerly-Rust reads exactly the environment variables Ledgerly-Java reads, under the same names: `SPRING_PROFILES_ACTIVE`, `SPRING_DATASOURCE_URL` (including the `jdbc:postgresql://` form), `SPRING_DATASOURCE_USERNAME`/`_PASSWORD`, `SPRING_SECURITY_OAUTH2_RESOURCESERVER_JWT_ISSUER_URI` and every `LEDGERLY_*` variable. It also serves Java's management endpoints under the same `/actuator/...` paths on port 8081. A reader will be surprised to find `SPRING_*` names in a Rust service. They are kept on purpose: swapping the image in a Compose file or deployment has to be the whole migration (ADR 0005).

## Consequences

- The profiles `local`, `test`, `prod` and `keycloak` keep their Java meaning.
- In `prod` nothing has a default, and the app refuses to start, listing every missing variable.
- The one operational difference: Java migrated the schema itself at startup, while Rust needs the Flyway step first (ADR 0006). On a database Java already migrated, swapping the image is enough.
- Rust-native names such as `DATABASE_URL` are not accepted as alternatives, so there is never a question of which one wins.
