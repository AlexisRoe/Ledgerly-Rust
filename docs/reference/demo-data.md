<!--
Extracted on 2026-11-12 from DevDataSeeder.java at the pinned commit:
https://github.com/AlexisRoe/Ledgerly-Java/blob/e2a13909634808b347d30b3dec4c3ffee6741c02/src/main/java/com/ledgerly/shared/seed/DevDataSeeder.java
Where this disagrees with that file, the file wins.
-->

# Demo data

What the `local` profile writes into an empty database at startup. The Bruno collection and the `http/` files rely on it.

## When

- Only when the profile is `local` and not `test`. Never under `keycloak`: the identity provider owns the accounts, so the seeder logs `Demo data skipped: accounts are managed by the identity provider` and stops.
- Skips completely if a team with id `00000000-0000-7000-8000-00000000a001` exists. That makes restarts idempotent.
- `now` is the application clock's current instant; `today` is `now`'s date in UTC.

## Values

**Only the team id is fixed.** Every other id is a fresh UUIDv7 generated at seeding time.

Users (`users`, `on conflict (email) do nothing`, then the id is read back by email):

| email | verified | password |
| --- | --- | --- |
| `alice@example.com` | true | `correct-horse-battery` |
| `bob@example.com` | true | `correct-horse-battery` |
| `carol@example.com` | true | `correct-horse-battery` |
| `dave@example.com` | false | `correct-horse-battery` |

The password is hashed **once** with the production password hasher (Argon2id, ADR 0005), and all four users get that same hash. `created_at = now`.

Team: `teams (id, name, created_at) = ('00000000-0000-7000-8000-00000000a001', 'Acme Travel', now)`.

Memberships (`joined_at = now`): alice `OWNER`, bob `APPROVER`, carol `MEMBER`. Dave is not a member.

Invitation: email `dave@example.com`, role `MEMBER`, status `PENDING`, `invited_by` = alice, `created_at = now`, `expires_at = now + 7 days`.

Expenses (`version = 0`, `created_at = updated_at = now`, no note):

| claimant | amount | currency | category | incurred_on | merchant | status |
| --- | --- | --- | --- | --- | --- | --- |
| carol | 38.40 | EUR | MEALS | today − 2 | Cafe Central | DRAFT |
| carol | 212.00 | EUR | TRAVEL | today − 6 | Rail Europe | SUBMITTED |
| carol | 49.99 | EUR | SOFTWARE | today − 9 | Editor Inc | APPROVED |
| carol | 980.00 | EUR | OFFICE | today − 12 | Desk Depot | REJECTED |
| carol | 75.00 | EUR | TRAVEL | today − 20 | City Taxi | REIMBURSED |
| bob | 19.90 | USD | OTHER | today − 1 | Parking | DRAFT |
| alice | 120.00 | EUR | MEALS | today − 4 | Team dinner | SUBMITTED |

Rejection of carol's REJECTED expense: `expense_rejections (expense_id, position, rejected_by, reason) = (<that expense>, 0, bob, 'Over the office budget; please get a quote first.')`.

The demo data bypasses the domain on purpose. It writes SQL directly, so the SUBMITTED, APPROVED and REIMBURSED expenses have no receipts, and no audit events or outbox rows are written.

## Log line

After seeding, at INFO:

```text
Demo data created: team 'Acme Travel' with alice (OWNER), bob (APPROVER), carol (MEMBER), dave (unverified, invited). Password for all: correct-horse-battery
```

This is the only log line that may contain a password: a published development password, and only under `local`. The log-hygiene test (M10.5) runs under the parity profile, where the seeder never runs.
