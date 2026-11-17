# Teaching notes

## Learner preferences
- The workspace lives in `learn/`, because the repo root already has the project's own `MISSION.md` and the domain `GLOSSARY.md`.
- Wants grouped sections, starting with the basics: setup and tools, syntax (comments, file structure, imports, folder structure), data types, control flow. More sections are welcome.
- Explain each Rust concept against its TypeScript counterpart first, then say where the analogy breaks. **TypeScript only, no Java comparisons** (decided 2026-11-17).
- Curriculum agreed on 2026-11-17 (`CURRICULUM.md`): ~79 short lessons, browser quizzes **plus** playground exercises, one commit per section on the `building-learning` branch (never pushed).
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
