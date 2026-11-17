# Apply Java's migrations unchanged, with Flyway

The schema is defined by Ledgerly-Java's Flyway scripts (`V1`–`V10`), vendored byte-for-byte and applied by the official Flyway container as a one-shot step before the app starts, not ported to `sqlx migrate`. Porting would let the schemas drift and would silently lose properties that live only in SQL, such as V8's revoked privileges and the trigger that makes the audit trail append-only.

## Consequences

- Framework tables created for Java (`BATCH_*` from Spring Batch, `event_publication` from Spring Modulith) exist in the Rust database too. Rust leaves `BATCH_*` unused and implements imports on `import_jobs`/`import_row_errors` directly.
- The app never runs DDL. At startup it checks that `flyway_schema_history` holds exactly the vendored migrations, by version and checksum, and refuses to start otherwise.
- Flyway runs with the same `SPRING_DATASOURCE_*` credentials the app uses, just as Java ran it in-process. V8's `REVOKE ... FROM current_user` therefore hits the app's own role exactly as it did in Java.
- Java migrated at startup; Rust needs the one-shot `flyway` service before the app (in Compose: `depends_on: condition: service_completed_successfully`). A database Java already migrated needs no extra step.
- A schema change is only possible by changing Ledgerly-Java first and re-vendoring (ADR 0007).
