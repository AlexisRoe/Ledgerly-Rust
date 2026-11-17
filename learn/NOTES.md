# Teaching notes

## Learner preferences
- The workspace lives in `learn/`, because the repo root already has the project's own `MISSION.md` and the domain `GLOSSARY.md`.
- Wants grouped sections, starting with the basics: setup and tools, syntax (comments, file structure, imports, folder structure), data types, control flow. More sections are welcome.
- Explain each Rust concept against its TypeScript counterpart first, then say where the analogy breaks. **TypeScript only, no Java comparisons** (decided 2026-11-17).
- Curriculum agreed on 2026-11-17 (`CURRICULUM.md`): ~79 short lessons, browser quizzes **plus** playground exercises, one commit per section on the `building-learning` branch. The learner allowed pushing this branch and updating PR #2.
- Part II (sections 15–22, lessons 80–128) requested the same day: "more in-depth knowledge in Rust itself"; approved as proposed and built in one go.
- Asked for all lessons to be built in one go, without waiting between lessons.

## Environment
- Rust is installed via rustup under `~/.cargo/bin`, which isn't on PATH in non-interactive shells: run `eval "$(mise env -s bash)"` first, or call `~/.cargo/bin/cargo`. Node is available through mise.
- VS Code is the editor (`code` is on PATH). Code captions link to `vscode://file/<repo>/<path>:<line>`.
- Chrome is installed, so headless screenshots work for checking pages.

## Authoring conventions (keep every lesson consistent)
- File: `lessons/NNNN-slug.html`; the slug and title must match `assets/course.js` (the manifest).
- Head: `assets/style.css`, then `course.js`, `code.js`, `quiz.js`. The body is `<main>` only; course.js adds the top bar, the ask box, the done button and prev/next.
- Order inside `<main>`: `p.box-label` (section), `h1`, `p.lede`, `div.goal`, `div.recall` (warm-up with `<details>`; skip it only in lesson 1), body sections (`h2`), `div.compare` for TS ↔ Rust, `div.ledgerly` for real code, `div.breaks` for "where the analogy breaks", `div.quiz` blocks (3–5), optional `div.exercise` (playground), `aside.source` (primary source), then `p.score[data-score]`.
- Code goes in `<script type="text/x-code" data-lang="rust|ts|toml|bash|json|text" data-src="repo/path" data-line="N" data-hl="2-3">`, so `<`, `>` and `&` stay raw. Inline `<code>` still needs `&lt;` / `&gt;` / `&amp;`.
- Quiz options: same word count, and as close to the same length as possible. The right one is marked `data-correct`. Options are shuffled unless the list has `data-fixed`.
- Links to other lessons: `<a data-lesson="12"></a>`, filled in by course.js.
- Playground exercise modules are `learn/playground/src/exNN_slug.rs` (NN = lesson number), with solutions in `learn/playground/solutions/` and checked by `verify-solutions.sh`.
- Real Ledgerly snippets must be copied verbatim (an elided part is marked `// …`), with the correct line numbers.

## Authoring helpers (in `.scratch/`, gitignored)
- `mk.sh`: `source` it, then `mk N slug "Title" <<'EOF' … EOF` wraps a `<main>` body in the standard head.
- `ex.py NAME < template`: writes `playground/src/NAME.rs` and `solutions/NAME.rs` from one template (`//<<`…`//>>` = solution
  body, replaced by `todo!()`; `//<<;` for `todo!();`; `//S` solution-only line; `//T` exercise-only line) and registers the module.
- `addsec.py < json`: appends a section (and optionally its cheat sheet) to the manifest in `assets/course.js`.
- `quizlens.py LO HI`, `sub.py FILE <<< 'old|||new'`, `urls.sh URL…`.

## Status (2026-11-17)
- Part I: 79 lessons, 15 reference pages. Part II: 49 lessons (80–128), 7 more reference pages, 30 more playground exercises, the capstone reference solution in `capstone/` (15 tests, clippy clean). Playground: 58 exercises; `verify-solutions.sh` runs 170 unit tests and 2 doc tests with solutions.
- Lessons were written before the learner took any of them, so they can't adapt to quiz results. Once the learner reports progress or confusion, write learning records and adjust (add remedial lessons rather than rewriting).
- Author tools: `python3 learn/check-lessons.py` checks manifest, snippet line numbers, quiz option lengths and sources.
- Part II performance numbers in lessons 108–109 come from `playground/src/bin/bench.rs` on the author machine (2026-11-17), release vs debug.
- Exercises 76–78 were simulated against the real code (compiler errors and failing tests quoted are real output from 2026-11-17). Re-run them if the expenses domain changes.
