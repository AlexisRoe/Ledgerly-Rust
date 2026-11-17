# tokio, axum and sqlx

The service runs on tokio, serves HTTP with axum and tower/tower-http middleware, and talks to Postgres through sqlx with compile-time-checked SQL. We rejected ORMs (Diesel, SeaORM) because they want to own the schema, which Flyway owns here (ADR 0006), and because explicit SQL keeps behaviour such as locking and idempotency visible. Tower layers map one-to-one onto Java's security filter chain, which keeps the port traceable.
