# Mission: Read and change Ledgerly-Rust, coming from TypeScript

## Why
You built Ledgerly-Rust (a drop-in Rust port of Ledgerly-Java) so you would have real backend code to learn from. You want to stop treating it as a black box: open any file, follow what it does, and make small changes yourself without an agent doing the typing.

## Success looks like
- You can explain how a Rust project is set up and what each tool in this repo does (rustup, cargo, workspaces, rustfmt, clippy, cargo-deny, nextest, just, mise).
- You can read any file in `crates/` and say what each line does, using TypeScript as the reference point where one exists.
- You can follow one HTTP request through Ledgerly (route → handler → application → domain → persistence) and name the crate and file for each step.
- You can make small changes (a new validation rule, a new enum variant, a changed message, a new test), let the compiler guide you, and get `just check` green again.

## Constraints
- Strong TypeScript background; Rust is new. Explain each new idea against TypeScript where that comparison holds, and say plainly where it does not (ownership, lifetimes).
- Learns from real code: examples come from this repository where possible.
- Self-paced; lessons are created up front, so they cannot adapt to quiz results between lessons.

## Out of scope
- Writing a Rust project from scratch, `unsafe`, writing procedural macros, embedded/WASM.
- Changing Ledgerly's behaviour, schema or contract (those come from Ledgerly-Java).
