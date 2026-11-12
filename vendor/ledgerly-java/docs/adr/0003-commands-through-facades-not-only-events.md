# Other modules may send commands through a facade; events are for reactions

The original rule was that writes across modules happen only through events. An Import has to create Draft expenses, though, and doing that through an event ("rows parsed, please create them") hides a command behind an event and loses the per-row results the import must report. We decided that a module may call another module's facade to ask it to change its own data (for example `ExpensesFacade.createDrafts`), and that events stay the way modules *react* to something that has already happened. What remains forbidden is touching another module's tables, repositories or `internal` classes.

## Consequences

- A facade method that writes data is a public command: it validates everything itself and never trusts the caller.
- The audit trail still sees every change, because the owning module publishes its own events after the command.
