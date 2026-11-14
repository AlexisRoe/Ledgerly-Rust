# Architecture Decision Records

Ledgerly-Rust is a drop-in reimplementation of [Ledgerly-Java](https://github.com/AlexisRoe/Ledgerly-Java/blob/e2a13909634808b347d30b3dec4c3ffee6741c02). The domain and architecture decisions recorded there apply here unchanged, and numbering continues from them so that an ADR number never means two different things:

- [0001 – Users referenced by id only](https://github.com/AlexisRoe/Ledgerly-Java/blob/e2a13909634808b347d30b3dec4c3ffee6741c02/docs/adr/0001-users-referenced-by-id-only.md) _(Java)_
- [0002 – Expenses asks receipts through an interface it owns](https://github.com/AlexisRoe/Ledgerly-Java/blob/e2a13909634808b347d30b3dec4c3ffee6741c02/docs/adr/0002-expenses-asks-receipts-through-an-interface-it-owns.md) _(Java)_
- [0003 – Commands through facades, events only for reactions](https://github.com/AlexisRoe/Ledgerly-Java/blob/e2a13909634808b347d30b3dec4c3ffee6741c02/docs/adr/0003-commands-through-facades-not-only-events.md) _(Java)_
- [0004 – CSRF: SameSite=Strict refresh cookie plus X-Requested-With](https://github.com/AlexisRoe/Ledgerly-Java/blob/e2a13909634808b347d30b3dec4c3ffee6741c02/docs/adr/0004-csrf-samesite-strict-plus-custom-header.md) _(Java)_

Rust-specific decisions start at 0005.
