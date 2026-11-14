-- Accounts managed by an external identity provider (Keycloak). Such a user has no password here, and is found
-- through the provider's stable "sub" claim; the id used everywhere else in Ledgerly stays our own (ADR 0001).
alter table users add column external_subject varchar(255);
alter table users add constraint users_external_subject_uk unique (external_subject);
alter table users alter column password_hash drop not null;
