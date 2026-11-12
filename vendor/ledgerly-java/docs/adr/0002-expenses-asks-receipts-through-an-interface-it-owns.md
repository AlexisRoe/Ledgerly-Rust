# Expenses asks about receipts through an interface it owns

Uploading a receipt must check that its expense is a Draft (receipts → expenses), and submitting an expense must check that its receipts are Accepted (expenses → receipts). Calling each other's facades would be a module cycle, which Spring Modulith rejects. We decided that the expenses module publishes a `ReceiptChecks` interface (a summary of receipt counts by scan status for one expense) and the receipts module implements it, so every dependency points from receipts to expenses.

## Considered Options

- **Expenses keeps its own copy of receipt statuses, updated from receipt events.** Rejected: the copy lags behind, so an expense could be submitted while a scan is still pending.
- **Merge receipts into expenses.** Rejected: the upload, storage and scanning concerns are big enough to deserve their own boundary, and that boundary is part of what this project teaches.
