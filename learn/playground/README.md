# Playground: hands-on exercises

A small standalone cargo project that goes with the lessons in `learn/lessons/`. It's not part of Ledgerly's
workspace (see the empty `[workspace]` table in `Cargo.toml`), so nothing here affects `just check`.

## How it works

Each exercise is a module `src/exNN_topic.rs`, where `NN` is the number of the lesson that introduces it. The
module has functions for you to finish, usually with a `todo!()` body, and tests at the bottom that fail until
your code is right. That's the feedback loop: edit, run, read, repeat.

```sh
cd learn/playground
cargo test ex16          # run only lesson 16's tests (the filter matches the module name)
cargo test ex16 -- --nocapture   # also show println! output
cargo run --bin hello    # binaries in src/bin/ run with --bin
```

A `todo!()` compiles but panics when called, so a fresh exercise fails with `not yet implemented`. Replace it
with your code.

Some lessons ask you to **uncomment a line to see a compile error**. While that line is uncommented, the whole
crate fails to compile, including other exercises. Read the error, then fix the line or comment it out again.

Stuck? `solutions/` has one possible answer per exercise, but try first, and ask your teacher (the agent) before
peeking. Getting it wrong with effort teaches more than reading a solution.

To reset an exercise: `git checkout -- src/exNN_topic.rs`.
