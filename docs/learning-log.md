# Learning log

One section per milestone, written by the implementing agent when the milestone is done, before it stops for review. This is **raw material for the lessons**, which another agent writes after M11. Write for that reader: they will have this file, the code and the vendored Java, but none of the conversation.

## Entry format

```md
## M<N> · <title>

- **Java → Rust:** one bullet per construct that changed shape. Name the Java file and the Rust file, and say what moved from runtime or tests into the type system (or the other way round).
  - e.g. `ExpenseStatus` + guard methods (`https://github.com/AlexisRoe/Ledgerly-Java/blob/e2a13909634808b347d30b3dec4c3ffee6741c02/src/main/java/com/ledgerly/expenses/internal/domain/Expense.java`) → `ExpenseStatus` enum with an exhaustive `match` in `Expense::apply` (`crates/expenses/src/domain/expense.rs`): adding a status no longer compiles until every transition decides what to do with it.
- **Rust features that carried the weight:** ownership, traits, lifetimes, async, macros…, with the file where each shows best.
- **Surprises and dead ends:** what did not work first and why, with links to the fixing commit.
- **Parity traps:** behaviour that had to be copied from Java exactly although the contract does not say so.
- **Decisions:** anything decided during the milestone that is not worth an ADR.
- **Open questions:** things the reviewer should look at or explain.
```

## M0 · Domain core

## M1 · First axum app

## M2 · Persist expenses

## M3 · Contract, validation, errors

## M4 · Paging and filtering

## M5 · Authentication

## M6 · Teams and authorization

## M7 · Receipts

## M8 · Batch work

## M9 · Events and audit

## M10 · Hardening and observability

## M11 · Ship it
