# One crate per module

Each Spring Modulith module from Ledgerly-Java (`identity`, `teams`, `expenses`, `receipts`, `imports`, `audit`, plus `shared`) becomes its own crate in a Cargo workspace, exposing only a facade and its events. A thin `app` crate wires them together. Crate visibility turns the module boundaries that Java checked in tests (ArchUnit, `ModularityTest`) into compile errors, and the dependency graph between crates must match Java's module arrows.
