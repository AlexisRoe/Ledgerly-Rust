# Wants depth in the language itself, beyond Ledgerly

After Part I was built, the learner asked whether finishing it would mean understanding Rust as a language, and then asked for lessons giving "more in-depth knowledge in Rust itself", as an experienced TypeScript backend developer. They approved Part II (sections 15–22, lessons 80–128) and asked for all of it to be built.

**Evidence:** the learner's requests on 2026-11-17, after the Part I pull request.

**Implications:** Part II assumes Part I and goes deeper: lifetimes and interior mutability, trait design, concurrency and async internals (the biggest gap for someone coming from Node's single thread), performance, API design, writing macros, `unsafe` in brief, and a capstone built from `cargo new`. The mission now includes writing a service from scratch (MISSION.md, Part II). Still no Java comparisons. As with Part I, lessons were written before the learner took them; adapt once they report progress.
