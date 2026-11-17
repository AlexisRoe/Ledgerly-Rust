# Mission: Read and change Ledgerly-Rust, coming from TypeScript

## Why
You built Ledgerly-Rust (a drop-in Rust port of Ledgerly-Java) so you would have real backend code to learn from. You want to stop treating it as a black box: open any file, follow what it does, and make small changes yourself without an agent doing the typing.

## Success looks like
- You can explain how a Rust project is set up and what each tool in this repo does (rustup, cargo, workspaces, rustfmt, clippy, cargo-deny, nextest, just, mise).
- You can read any file in `crates/` and say what each line does, using TypeScript as the reference point where one exists.
- You can follow one HTTP request through Ledgerly (route → handler → application → domain → persistence) and name the crate and file for each step.
- You can make small changes (a new validation rule, a new enum variant, a changed message, a new test), let the compiler guide you, and get `just check` green again.

## Part II: Rust in depth (added 2026-11-17, at the learner's request)
- You can explain what's underneath: lifetimes you write yourself, interior mutability, trait design, `Send`/`Sync`,
  how futures and `Pin` work, cancellation, memory layout and allocation costs.
- You can design Rust code: error types, parse-don't-validate types, builders, crate boundaries, documentation.
- You can build a small concurrent service from `cargo new` (the capstone in section 22) and review it against
  production rules.

## Constraints
- Strong TypeScript background; Rust is new. Explain each new idea against TypeScript where that comparison holds, and say plainly where it does not (ownership, lifetimes).
- Learns from real code: examples come from this repository where possible.
- Self-paced; lessons are created up front, so they cannot adapt to quiz results between lessons.

## Out of scope
- Embedded/WASM, writing procedural macros (Part II explains them; it doesn't build one), writing `unsafe` code
  (Part II teaches what it is and how to judge it). Writing a project from scratch moved into scope with Part II.
- Changing Ledgerly's behaviour, schema or contract (those come from Ledgerly-Java).
