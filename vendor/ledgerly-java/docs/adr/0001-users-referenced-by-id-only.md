# Users are referenced by id only; account deletion is out of scope

Ledgerly's audit trail is append-only and expenses keep their claimant forever, which conflicts with erasing a person on request (GDPR). We do not offer account deletion in the core scope. Instead, every other record (expenses, receipts, imports, the audit trail) refers to a user only by their id, never by email or name, so a later "erase user" feature only has to anonymise the single user record and nothing else.

## Consequences

- Audit event details and logs must never copy emails or names in; anything shown to people resolves names at read time.
- Breaking this rule once (for example, storing an email in an audit event's details) makes later erasure a data migration across append-only data.
