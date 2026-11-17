// Code blocks for the lessons.
//
// Authoring: write code inside a raw-text script tag, so `<T>` and `&str` need no HTML escaping:
//
//   <script type="text/x-code" data-lang="rust" data-src="crates/x/src/y.rs" data-line="12" data-hl="3,5-6">
//     fn main() {}
//   </script>
//
// data-lang:  rust | ts | toml | bash | json | text   (default rust)
// data-src:   optional repo-relative path; adds a caption that opens the file in VS Code
//             (on the published site: on GitHub)
// data-line:  optional first line number of the excerpt in that file
// data-label: optional caption text (defaults to the language name)
// data-hl:    optional lines to highlight, 1-based within the excerpt
//
// The block is dedented, trimmed and highlighted, then replaced by <figure class="code">.

(function () {
  "use strict";

  const RUST_KEYWORDS = [
    "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum", "extern", "false", "fn",
    "for", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub", "ref", "return", "self", "Self",
    "static", "struct", "super", "trait", "true", "type", "unsafe", "use", "where", "while",
  ];
  const TS_KEYWORDS = [
    "abstract", "any", "as", "async", "await", "boolean", "break", "case", "catch", "class", "const", "continue",
    "default", "delete", "do", "else", "enum", "export", "extends", "false", "finally", "for", "from", "function",
    "if", "implements", "import", "in", "instanceof", "interface", "keyof", "let", "never", "new", "null", "number",
    "of", "private", "protected", "public", "readonly", "return", "satisfies", "static", "string", "super", "switch",
    "this", "throw", "true", "try", "type", "typeof", "undefined", "unknown", "var", "void", "while", "bigint",
  ];

  const words = (list) => "\\b(?:" + list.join("|") + ")\\b";

  // Each grammar is a list of [className, regexSource]; the first alternative that matches at a position wins.
  const GRAMMARS = {
    rust: [
      ["c", "//[^\\n]*|/\\*[\\s\\S]*?\\*/"],
      ["s", "b?r(#+)\"[\\s\\S]*?\"\\1|b?r\"[^\"]*\"|b?\"(?:\\\\[\\s\\S]|[^\"\\\\])*\""],
      ["s", "b?'(?:\\\\(?:x[0-9a-fA-F]{2}|u\\{[0-9a-fA-F]+\\}|.)|[^'\\\\\\n])'"],
      ["l", "'[a-zA-Z_]\\w*\\b"],
      ["a", "#!?\\[(?:[^\\[\\]]|\\[[^\\]]*\\])*\\]"],
      ["m", "\\b[a-z_][a-zA-Z0-9_]*!(?=[(\\[{\\s])"],
      ["n", "\\b(?:0x[0-9a-fA-F_]+|0b[01_]+|\\d[\\d_]*(?:\\.\\d[\\d_]*)?(?:[eE][+-]?\\d+)?)(?:[iu](?:8|16|32|64|128|size)|f32|f64)?\\b"],
      ["k", words(RUST_KEYWORDS)],
      ["t", "\\b(?:[A-Z][A-Za-z0-9_]*|i8|i16|i32|i64|i128|isize|u8|u16|u32|u64|u128|usize|f32|f64|bool|char|str)\\b"],
    ],
    ts: [
      ["c", "//[^\\n]*|/\\*[\\s\\S]*?\\*/"],
      ["s", "\"(?:\\\\[\\s\\S]|[^\"\\\\])*\"|'(?:\\\\[\\s\\S]|[^'\\\\])*'|`(?:\\\\[\\s\\S]|[^`\\\\])*`"],
      ["a", "@[A-Za-z_]\\w*"],
      ["n", "\\b\\d[\\d_]*(?:\\.\\d+)?n?\\b"],
      ["k", words(TS_KEYWORDS)],
      ["t", "\\b[A-Z][A-Za-z0-9_]*\\b"],
    ],
    toml: [
      ["c", "#[^\\n]*"],
      ["a", "^[ \\t]*\\[\\[?[^\\]\\n]+\\]\\]?"],
      ["s", "\"(?:\\\\.|[^\"\\\\])*\"|'[^'\\n]*'"],
      ["k", "^[ \\t]*[A-Za-z0-9_.\"-]+(?=\\s*=)"],
      ["n", "\\b(?:true|false|\\d[\\d_.]*)\\b"],
    ],
    bash: [
      ["c", "(?:^|\\s)#[^\\n]*"],
      ["s", "\"(?:\\\\.|[^\"\\\\])*\"|'[^']*'"],
      ["k", "^\\$(?= )"],
      ["n", "(?:^|\\s)--?[A-Za-z][\\w-]*"],
    ],
    json: [
      ["k", "\"(?:\\\\.|[^\"\\\\])*\"(?=\\s*:)"],
      ["s", "\"(?:\\\\.|[^\"\\\\])*\""],
      ["n", "-?\\b\\d+(?:\\.\\d+)?(?:[eE][+-]?\\d+)?\\b|\\b(?:true|false|null)\\b"],
    ],
    text: [],
  };
  GRAMMARS.typescript = GRAMMARS.ts;
  GRAMMARS.sh = GRAMMARS.bash;
  GRAMMARS.shell = GRAMMARS.bash;

  const LABELS = { rust: "Rust", ts: "TypeScript", typescript: "TypeScript", toml: "TOML", bash: "Shell",
    sh: "Shell", shell: "Shell", json: "JSON", text: "Output" };

  const escapeHtml = (s) => s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");

  function compile(lang) {
    const grammar = GRAMMARS[lang] || [];
    if (!grammar.length) return null;
    // Wrap each rule in a named-ish capture by position; inner groups are offset, so track them.
    let source = "";
    const classes = [];
    let groupIndex = 1;
    grammar.forEach(([cls, re], i) => {
      const inner = new RegExp(re + "|").exec("").length - 1; // number of groups inside the rule
      // Backreferences such as \1 inside a rule must be shifted by the groups that come before it.
      const shifted = re.replace(/\\(\d)/g, (_, d) => "\\" + (Number(d) + groupIndex));
      source += (i ? "|" : "") + "(" + shifted + ")";
      classes.push({ cls, group: groupIndex });
      groupIndex += 1 + inner;
    });
    return { re: new RegExp(source, "gm"), classes };
  }

  const compiled = {};

  function highlight(code, lang) {
    if (!(lang in compiled)) compiled[lang] = compile(lang);
    const g = compiled[lang];
    if (!g) return escapeHtml(code);
    let out = "";
    let last = 0;
    g.re.lastIndex = 0;
    let m;
    while ((m = g.re.exec(code)) !== null) {
      if (m[0] === "") { g.re.lastIndex++; continue; }
      const hit = g.classes.find((c) => m[c.group] !== undefined);
      out += escapeHtml(code.slice(last, m.index));
      out += '<span class="tok-' + hit.cls + '">' + escapeHtml(m[0]) + "</span>";
      last = m.index + m[0].length;
    }
    return out + escapeHtml(code.slice(last));
  }

  function dedent(text) {
    const lines = text.replace(/\t/g, "    ").split("\n");
    while (lines.length && lines[0].trim() === "") lines.shift();
    while (lines.length && lines[lines.length - 1].trim() === "") lines.pop();
    const indents = lines.filter((l) => l.trim()).map((l) => l.match(/^ */)[0].length);
    const cut = indents.length ? Math.min(...indents) : 0;
    return lines.map((l) => l.slice(cut)).join("\n");
  }

  function parseLines(spec) {
    const set = new Set();
    (spec || "").split(",").map((s) => s.trim()).filter(Boolean).forEach((part) => {
      const [a, b] = part.split("-").map(Number);
      for (let i = a; i <= (b || a); i++) set.add(i);
    });
    return set;
  }

  function repoRoot() {
    const path = decodeURIComponent(location.pathname);
    const at = path.lastIndexOf("/learn/");
    return at >= 0 ? path.slice(0, at) : null;
  }

  // The published site (scripts/build-learn-site.py) sets this to the repository's GitHub URL at the deployed commit.
  function sourceBase() {
    const meta = document.querySelector('meta[name="ledgerly-source-base"]');
    return meta ? meta.content : null;
  }

  function sourceLink(src, line) {
    const root = repoRoot();
    const base = sourceBase();
    const text = src + (line ? ":" + line : "");
    const a = document.createElement("a");
    a.textContent = text;
    if (base) {
      a.title = "Open on GitHub";
      a.href = base + src + (line ? "#L" + line : "");
    } else {
      a.title = "Open in VS Code";
      a.href = root && location.protocol === "file:" ? "vscode://file" + root + "/" + src + (line ? ":" + line : "")
        : "../../" + src;
    }
    return a;
  }

  function render(script) {
    const lang = (script.dataset.lang || "rust").toLowerCase();
    const code = dedent(script.textContent);
    const hl = parseLines(script.dataset.hl);
    let html = highlight(code, lang);
    if (hl.size) {
      html = html.split("\n").map((l, i) => (hl.has(i + 1) ? '<span class="hl-line">' + (l || " ") + "</span>" : l))
        .join("\n");
    }
    const figure = document.createElement("figure");
    figure.className = "code lang-" + (lang === "typescript" ? "ts" : lang);
    const caption = document.createElement("figcaption");
    const label = document.createElement("span");
    label.className = "lang";
    label.textContent = script.dataset.label || LABELS[lang] || lang;
    caption.appendChild(label);
    if (script.dataset.src) caption.appendChild(sourceLink(script.dataset.src, script.dataset.line));
    figure.appendChild(caption);
    const pre = document.createElement("pre");
    const codeEl = document.createElement("code");
    codeEl.innerHTML = html;
    pre.appendChild(codeEl);
    figure.appendChild(pre);
    script.replaceWith(figure);
  }

  function renderInlineSources() {
    // <a class="src" data-src="crates/…" data-line="12"></a> becomes a VS Code link to that file.
    document.querySelectorAll("a.src[data-src]").forEach((a) => {
      const link = sourceLink(a.dataset.src, a.dataset.line);
      a.href = link.href;
      a.title = link.title;
      if (!a.textContent.trim()) {
        const code = document.createElement("code");
        code.textContent = link.textContent;
        a.appendChild(code);
      }
    });
  }

  window.LedgerlyCode = { highlight, render, sourceLink };

  document.addEventListener("DOMContentLoaded", () => {
    document.querySelectorAll('script[type="text/x-code"]').forEach(render);
    renderInlineSources();
  });
})();
