// The course manifest and the chrome around every lesson: top bar, prev/next links, the "ask your teacher"
// reminder, a "mark as done" button, and progress (stored in localStorage).
//
// Lessons are numbered 1..N; a lesson file is lessons/<NNNN>-<slug>.html. The index page renders the table of
// contents from this manifest into <div id="toc">. Add a lesson here and every page picks it up.

(function () {
  "use strict";

  const SECTIONS = [
    { part: "Part I · Reading and changing Ledgerly", title: "Setting up and the toolbox", blurb: "How a Rust project is built, and what every tool in this repo is for.",
      ref: "tools.html", lessons: [
        [1, "rust-vs-typescript", "Rust vs TypeScript: the big picture"],
        [2, "rustup-and-toolchains", "rustup and toolchains"],
        [3, "cargo-is-npm-tsc-jest", "Cargo is npm + tsc + jest"],
        [4, "workspaces", "Workspaces"],
        [5, "quality-tools", "rustfmt, clippy, rust-analyzer, cargo-deny, nextest"],
        [6, "just-and-the-workflow", "just and the repo's workflow"],
        [7, "review-toolbox", "Review: the toolbox"],
      ] },
    { title: "Basic syntax and code organisation", blurb: "Reading a Rust file: functions, comments, modules, imports, folders.",
      ref: "syntax.html", lessons: [
        [8, "hello-rust", "Hello, Rust: fn main, let, mut, println!"],
        [9, "statements-and-expressions", "Statements, expressions and the missing semicolon"],
        [10, "comments-and-docs", "Comments and doc comments"],
        [11, "files-become-modules", "Files become modules"],
        [12, "imports-with-use", "Imports with use"],
        [13, "visibility-and-reexports", "Visibility and re-exports"],
        [14, "folder-structure", "Folder structure of a crate and of this repo"],
        [15, "review-syntax", "Review: syntax and modules"],
      ] },
    { title: "Data types", blurb: "From TypeScript's handful of types to Rust's precise ones.",
      ref: "types.html", lessons: [
        [16, "numbers", "Numbers: one number becomes many"],
        [17, "bool-char-inference", "bool, char and type inference"],
        [18, "strings", "Strings: String vs &str"],
        [19, "tuples-arrays-slices-vec", "Tuples, arrays, slices and Vec"],
        [20, "structs", "Structs: like interfaces, plus data"],
        [21, "impl-blocks", "impl blocks: methods and associated functions"],
        [22, "enums-plain", "Enums I: plain variants"],
        [23, "enums-with-data", "Enums II: variants with data"],
        [24, "option", "Option: no null, no undefined"],
        [25, "result", "Result: errors as values"],
        [26, "newtypes", "Newtypes: branded types done right"],
        [27, "generic-types", "Generic types you'll see everywhere"],
        [28, "review-data-types", "Review: data types"],
      ] },
    { title: "Control flow", blurb: "if, loops and match, which is like a switch that checks every case.",
      ref: "control-flow.html", lessons: [
        [29, "if-is-an-expression", "if is an expression"],
        [30, "loops", "Loops: loop, while, for"],
        [31, "match", "match: a switch that checks everything"],
        [32, "patterns-destructuring", "Patterns and destructuring"],
        [33, "if-let-let-else", "if let, let … else, while let, matches!"],
        [34, "early-exit-question-mark", "Early exit: return and ?"],
        [35, "review-control-flow", "Review: control flow"],
      ] },
    { title: "Ownership and borrowing", blurb: "The part with no TypeScript equivalent, and the one that matters most.",
      ref: "ownership.html", lessons: [
        [36, "why-ownership", "Why ownership: memory without a GC"],
        [37, "move-semantics", "Move semantics"],
        [38, "copy-and-clone", "Copy and Clone"],
        [39, "borrowing", "Borrowing: & and &mut"],
        [40, "borrow-checker-errors", "Reading the borrow checker's errors"],
        [41, "lifetimes", "Lifetimes, enough to read them"],
        [42, "box-rc-arc", "Shared ownership: Box, Rc, Arc"],
        [43, "review-ownership", "Review: ownership"],
      ] },
    { title: "Functions, closures and iterators", blurb: "Signatures, arrow functions' cousins, and array methods done lazily.",
      ref: "functions.html", lessons: [
        [44, "function-signatures", "Function signatures in depth"],
        [45, "closures", "Closures vs arrow functions"],
        [46, "iterators", "Iterators vs array methods"],
        [47, "collecting-results", "Collecting into Result and Option"],
        [48, "review-functions", "Review: functions and iterators"],
      ] },
    { title: "Traits and generics", blurb: "Interfaces you implement explicitly, and generics with constraints.",
      ref: "traits.html", lessons: [
        [49, "traits", "Traits are interfaces, but nominal"],
        [50, "derive-standard-traits", "derive and the standard traits"],
        [51, "conversions", "Conversions: From, Into, TryFrom, FromStr, Display"],
        [52, "generics-bounds", "Generics with trait bounds"],
        [53, "trait-objects", "Trait objects: dyn Trait"],
        [54, "review-traits", "Review: traits and generics"],
      ] },
    { title: "Error handling the Ledgerly way", blurb: "No exceptions: errors are values, typed and converted by ?.",
      ref: "errors.html", lessons: [
        [55, "question-mark-and-from", "? and From: errors that convert themselves"],
        [56, "thiserror", "Custom errors with thiserror"],
        [57, "panics-vs-errors", "Panics vs errors, and why unwrap is banned"],
        [58, "domain-error-to-http", "From domain error to HTTP problem"],
        [59, "review-errors", "Review: error handling"],
      ] },
    { title: "Macros and attributes", blurb: "Reading the ! and #[…] that are everywhere.",
      ref: "macros.html", lessons: [
        [60, "attributes", "Attributes: #[…] and #![…]"],
        [61, "macros", "Macros you'll meet"],
      ] },
    { title: "Async Rust", blurb: "Futures vs Promises, and the runtime Node gives you for free.",
      ref: "async.html", lessons: [
        [62, "futures-vs-promises", "Futures vs Promises"],
        [63, "tokio", "Tokio: the runtime"],
        [64, "sharing-state-across-tasks", "Sharing state across tasks"],
      ] },
    { title: "Testing", blurb: "Tests next to the code, tests beside it, and the black-box parity suite.",
      ref: "testing.html", lessons: [
        [65, "unit-tests", "Unit tests next to the code"],
        [66, "integration-doc-property-tests", "Integration, doc and property tests"],
        [67, "ledgerly-test-layers", "Ledgerly's test layers"],
      ] },
    { title: "The libraries Ledgerly is built on", blurb: "serde, sqlx, axum and friends, each compared with its Node counterpart.",
      ref: "libraries.html", lessons: [
        [68, "serde", "serde: JSON in and out"],
        [69, "sqlx", "sqlx: SQL checked at compile time"],
        [70, "axum-and-routes", "axum and Ledgerly's Routes"],
        [71, "tracing-metrics-secrecy", "tracing, metrics and secrecy"],
      ] },
    { title: "Reading Ledgerly end to end", blurb: "Putting it together: the crate map, one request, and startup.",
      ref: "ledgerly-map.html", lessons: [
        [72, "crate-map", "The crate map"],
        [73, "following-a-request", "Following one request: submit an expense"],
        [74, "startup", "Startup: from main to a listening server"],
      ] },
    { title: "Making changes", blurb: "The capstone: change real code and let the compiler guide you.",
      ref: "change-loop.html", lessons: [
        [75, "the-change-loop", "The change loop"],
        [76, "exercise-domain-rule", "Exercise: change a domain rule and its test"],
        [77, "exercise-enum-variant", "Exercise: add an enum variant"],
        [78, "exercise-test-and-helper", "Exercise: add a test, then a helper"],
        [79, "where-next", "Where to go next"],
      ] },
    { part: "Part II · Rust in depth", title: "Ownership and lifetimes in depth", blurb: "Writing lifetimes, borrowing structs, 'static, interior mutability, Cow and Drop.",
      ref: "ownership-deep.html", lessons: [
        [80, "writing-lifetimes", "Writing lifetime annotations"],
        [81, "structs-that-borrow", "Structs that borrow"],
        [82, "static-lifetimes", "'static and T: 'static"],
        [83, "borrowing-in-practice", "Borrowing in practice"],
        [84, "interior-mutability", "Interior mutability"],
        [85, "cow", "Cow: borrow when you can"],
        [86, "drop-raii-take", "Drop, RAII and mem::take"],
        [87, "review-ownership-deep", "Review: ownership and lifetimes in depth"],
      ] },
    { title: "Traits in depth", blurb: "Associated types, writing iterators, blanket impls, coherence, dyn compatibility, conversions, operators, typestate.",
      ref: "traits-deep.html", lessons: [
        [88, "associated-types", "Associated types vs generic parameters"],
        [89, "iterators-by-hand", "Implementing Iterator yourself"],
        [90, "default-blanket-extension", "Default methods, blanket impls, extension traits"],
        [91, "orphan-rule", "Coherence: the orphan rule"],
        [92, "dyn-compatibility", "dyn compatibility and async in traits"],
        [93, "asref-borrow-deref", "AsRef, Borrow, Deref and flexible parameters"],
        [94, "comparison-and-operators", "Comparison and operator traits"],
        [95, "typestate", "Typestate: states as types"],
        [96, "review-traits-deep", "Review: traits in depth"],
      ] },
    { title: "Concurrency and async internals", blurb: "Threads, Send and Sync, locks and atomics, channels, how futures and Pin work, cancellation, blocking.",
      ref: "concurrency.html", lessons: [
        [97, "threads", "Threads and scoped threads"],
        [98, "send-and-sync", "Send and Sync, really"],
        [99, "shared-state", "Shared mutable state"],
        [100, "channels-and-actors", "Channels and the actor pattern"],
        [101, "inside-a-future", "Inside a future: poll and Waker"],
        [102, "pin", "Pin, just enough"],
        [103, "cancellation", "Cancellation and structured concurrency"],
        [104, "blocking-in-async", "Blocking inside async"],
        [105, "review-concurrency", "Review: concurrency and async internals"],
      ] },
    { title: "Memory and performance", blurb: "Sizes and layout, hidden allocations, what generics and dyn cost, and how to measure.",
      ref: "performance.html", lessons: [
        [106, "stack-heap-layout", "Stack, heap and layout"],
        [107, "allocations", "Where allocations hide"],
        [108, "cost-of-abstractions", "Generics vs dyn: what abstractions cost"],
        [109, "measuring", "Measuring: release builds, benchmarks, profiles"],
        [110, "review-performance", "Review: memory and performance"],
      ] },
    { title: "Designing Rust code", blurb: "Error design, parse-don't-validate, constructors and builders, API conventions, crate boundaries, docs.",
      ref: "design.html", lessons: [
        [111, "error-design", "Error design: libraries vs applications"],
        [112, "parse-dont-validate", "Parse, don't validate, in depth"],
        [113, "constructors-and-builders", "Constructors, Default and builders"],
        [114, "apis-others-enjoy", "APIs others enjoy"],
        [115, "crate-boundaries", "Module and crate boundaries"],
        [116, "documentation-as-code", "Documentation as code"],
        [117, "review-design", "Review: designing Rust code"],
      ] },
    { title: "Writing macros", blurb: "macro_rules! from scratch, recursion and internal rules, procedural macros, and when not to.",
      ref: "macros-unsafe.html", lessons: [
        [118, "macro-rules", "macro_rules! from scratch"],
        [119, "macro-patterns", "Repetition, internal rules and hygiene"],
        [120, "procedural-macros", "Procedural macros, and when not to write one"],
      ] },
    { title: "unsafe, briefly", blurb: "What unsafe allows, why Ledgerly forbids it, and how to judge it in dependencies.",
      ref: "macros-unsafe.html", lessons: [
        [121, "what-unsafe-allows", "What unsafe allows"],
        [122, "unsafe-in-dependencies", "Reading unsafe in dependencies"],
      ] },
  ];

  const REFERENCES = [
    ["glossary.html", "Glossary"],
    ["tools.html", "Tools and commands"],
    ["syntax.html", "Syntax: TypeScript ↔ Rust"],
    ["types.html", "Data types"],
    ["control-flow.html", "Control flow"],
    ["ownership.html", "Ownership rules"],
    ["functions.html", "Functions, closures, iterators"],
    ["traits.html", "Traits and generics"],
    ["errors.html", "Error handling"],
    ["macros.html", "Macros and attributes"],
    ["async.html", "Async"],
    ["testing.html", "Testing"],
    ["libraries.html", "Libraries"],
    ["ledgerly-map.html", "Ledgerly map"],
    ["change-loop.html", "The change loop"],
    ["ownership-deep.html", "Ownership and lifetimes in depth"],
    ["traits-deep.html", "Traits in depth"],
    ["concurrency.html", "Concurrency and async internals"],
    ["performance.html", "Memory and performance"],
    ["design.html", "Designing Rust code"],
    ["macros-unsafe.html", "Writing macros, and unsafe"],
  ];

  const LESSONS = [];
  SECTIONS.forEach((section, si) => {
    section.lessons.forEach(([n, slug, title]) => {
      LESSONS.push({ n, slug, title, section: si, file: String(n).padStart(4, "0") + "-" + slug + ".html" });
    });
  });

  const KEY = "ledgerly-learn:done:";
  const isDone = (n) => {
    try { return localStorage.getItem(KEY + n) === "1"; } catch (e) { return false; }
  };
  const setDone = (n, value) => {
    try { value ? localStorage.setItem(KEY + n, "1") : localStorage.removeItem(KEY + n); } catch (e) { /* ignore */ }
  };

  function el(tag, attrs, ...children) {
    const e = document.createElement(tag);
    Object.entries(attrs || {}).forEach(([k, v]) => {
      if (k === "class") e.className = v; else if (k === "html") e.innerHTML = v; else e.setAttribute(k, v);
    });
    children.flat().forEach((c) => e.append(c));
    return e;
  }

  function currentLesson() {
    const file = decodeURIComponent(location.pathname.split("/").pop() || "");
    const m = file.match(/^(\d{4})-/);
    return m ? LESSONS.find((l) => l.n === Number(m[1])) : null;
  }

  let doneButton = null;

  function decorateLesson(lesson) {
    const main = document.querySelector("main");
    if (!main) return;
    const section = SECTIONS[lesson.section];
    document.title = lesson.n + ". " + lesson.title + " · Rust for TypeScript developers";

    const top = el("nav", { class: "course-top" },
      el("span", {}, el("a", { href: "../index.html" }, "Rust for TS devs"), " · ",
        "Section " + (lesson.section + 1) + ": " + section.title),
      el("span", {}, "Lesson " + lesson.n + " / " + LESSONS.length));
    main.prepend(top);

    const ask = el("aside", { class: "ask" },
      el("span", { class: "box-label" }, "Ask your teacher"),
      el("p", { html: "Something unclear, or want another example from Ledgerly? Ask the agent in your Claude Code " +
        "session, for example: <em>“In lesson " + lesson.n + ", why does …?”</em> Follow-up questions are the " +
        "fastest way to fill the gaps a page can't anticipate." }));

    doneButton = el("button", { class: "done-button", type: "button" });
    const syncButton = () => {
      const d = isDone(lesson.n);
      doneButton.setAttribute("aria-pressed", String(d));
      doneButton.textContent = d ? "✓ Done" : "Mark as done";
    };
    doneButton.addEventListener("click", () => { setDone(lesson.n, !isDone(lesson.n)); syncButton(); });
    syncButton();

    const prev = LESSONS[LESSONS.indexOf(lesson) - 1];
    const next = LESSONS[LESSONS.indexOf(lesson) + 1];
    const link = (l, cls, label) => l
      ? el("div", { class: cls }, el("small", {}, label), el("a", { href: l.file }, l.n + ". " + l.title))
      : el("div", { class: cls });
    const bottom = el("nav", { class: "course-bottom" },
      link(prev, "prev", "← Previous"),
      el("div", { class: "home" }, doneButton, el("small", {}, el("a", { href: "../index.html" }, "Contents"),
        " · ", el("a", { href: "../reference/" + section.ref }, "Cheat sheet"))),
      link(next, "next", "Next →"));

    main.append(ask, bottom);
  }

  function decorateReference() {
    const main = document.querySelector("main");
    if (!main || !document.body.classList.contains("reference")) return;
    main.prepend(el("nav", { class: "course-top" },
      el("span", {}, el("a", { href: "../index.html" }, "Rust for TS devs"), " · Reference"),
      el("span", {}, el("a", { href: "../index.html#refs" }, "All cheat sheets"))));
  }

  function renderToc() {
    const toc = document.getElementById("toc");
    if (!toc) return;
    SECTIONS.forEach((section, si) => {
      const list = el("ol");
      section.lessons.forEach(([n]) => {
        const l = LESSONS.find((x) => x.n === n);
        list.append(el("li", { class: l.slug.startsWith("review-") ? "review" : "" },
          el("span", { class: "num" }, String(n).padStart(2, "0")),
          el("a", { href: "lessons/" + l.file }, l.title),
          el("span", { class: "tick" }, isDone(n) ? "✓" : "")));
      });
      if (section.part) toc.append(el("h2", { class: "toc-part" }, section.part));
      toc.append(el("section", { class: "toc-section" },
        el("h2", {}, (si + 1) + " · " + section.title),
        el("p", { class: "blurb" }, section.blurb, " ",
          el("a", { href: "reference/" + section.ref }, "Cheat sheet →")),
        list));
    });
    const progress = document.getElementById("progress");
    if (progress) {
      const done = LESSONS.filter((l) => isDone(l.n)).length;
      progress.textContent = done + " of " + LESSONS.length + " lessons done";
    }
  }

  window.LedgerlyCourse = {
    SECTIONS, LESSONS, REFERENCES,
    markDone() {
      const l = currentLesson();
      if (!l) return;
      setDone(l.n, true);
      if (doneButton) { doneButton.setAttribute("aria-pressed", "true"); doneButton.textContent = "✓ Done"; }
    },
    lessonHref(n) { const l = LESSONS.find((x) => x.n === n); return l ? l.file : "#"; },
  };

  document.addEventListener("DOMContentLoaded", () => {
    const lesson = currentLesson();
    if (lesson) decorateLesson(lesson);
    decorateReference();
    renderToc();
    // <a data-lesson="12">…</a> becomes a link to lesson 12, with its title if the link is empty.
    document.querySelectorAll("a[data-lesson]").forEach((a) => {
      const l = LESSONS.find((x) => x.n === Number(a.dataset.lesson));
      if (!l) return;
      const inLessons = location.pathname.includes("/lessons/");
      a.href = (inLessons ? "" : location.pathname.includes("/reference/") ? "../lessons/" : "lessons/") + l.file;
      if (!a.textContent.trim()) a.textContent = "Lesson " + l.n + ": " + l.title;
    });
  });
})();
