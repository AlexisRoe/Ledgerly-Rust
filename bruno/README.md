# Bruno collection

API checks for Ledgerly, written for [Bruno](https://www.usebruno.com/) (open this folder as a collection) and
runnable without the app from the command line.

```sh
make up          # the application with demo data (docs/deployment.md, "Demo data")
make api-test    # runs every request and its assertions
```

Environment `local` holds the base URL, the demo team and the demo password. Folders run in order and pass values on
(tokens, ids, ETags) through variables:

| Folder | What it proves |
| --- | --- |
| `00-auth` | login for each demo user; a wrong password and an unknown email give the same 401; no token and a garbage token give 401 |
| `01-expenses` | create, read, edit, delete a draft; invalid input (400), missing or stale `If-Match` (428, 412), non-member gets 404, others' drafts cannot be edited (403) |
| `02-reading` | visibility by role, filters, paging limits, unknown sort field, CSV export, audit trail for owners only |
| `03-lifecycle` | a full run: create, upload a receipt, malware scan, submit, approve, reimburse, and the refusals on the way (needs ClamAV running) |
| `04-teams` | who may invite, who gets 404 |
| `05-security` | hardening headers, over-long input |

The lifecycle creates its own expense each time, so the collection can be run repeatedly. Requests use the demo users
only: run it against `make up`, never against a shared or production system.
