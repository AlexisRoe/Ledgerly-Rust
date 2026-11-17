#!/usr/bin/env python3
"""Checks the course pages: for authors, not learners.

- every lesson in assets/course.js exists as lessons/NNNN-slug.html, and nothing else is there;
- every data-src path exists in the repo;
- with data-line, the excerpt's first line equals that line of the file, and every other line of the excerpt
  (except `// …` / `# …` elisions) appears in the file;
- quiz options within one question have the same number of words (warning only);
- every lesson has a primary source and at least one quiz.

Run: python3 learn/check-lessons.py
"""

import html
import re
import sys
from html.parser import HTMLParser
from pathlib import Path

LEARN = Path(__file__).resolve().parent
ROOT = LEARN.parent


class Page(HTMLParser):
    def __init__(self):
        super().__init__(convert_charrefs=True)
        self.blocks = []  # (attrs, text)
        self._in_code = None
        self.quizzes = []  # list of list of option texts
        self._in_options = False
        self._in_li = False
        self._li_text = ""
        self.has_source = False
        self.fixed = set()

    def handle_starttag(self, tag, attrs):
        a = dict(attrs)
        if tag == "script" and a.get("type") == "text/x-code":
            self._in_code = (a, [])
        if tag == "ul" and "options" in (a.get("class") or ""):
            self._in_options = True
            self.quizzes.append([])
            if "data-fixed" in a:
                self.fixed.add(len(self.quizzes))
        if tag == "li" and self._in_options:
            self._in_li = True
            self._li_text = ""
        if tag == "aside" and "source" in (a.get("class") or ""):
            self.has_source = True

    def handle_endtag(self, tag):
        if tag == "script" and self._in_code is not None:
            self.blocks.append((self._in_code[0], "".join(self._in_code[1])))
            self._in_code = None
        if tag == "li" and self._in_li:
            self.quizzes[-1].append(self._li_text.strip())
            self._in_li = False
        if tag == "ul" and self._in_options:
            self._in_options = False

    def handle_data(self, data):
        if self._in_code is not None:
            self._in_code[1].append(data)
        if self._in_li:
            self._li_text += data


def dedent(text):
    lines = text.replace("\t", "    ").split("\n")
    while lines and not lines[0].strip():
        lines.pop(0)
    while lines and not lines[-1].strip():
        lines.pop()
    indents = [len(l) - len(l.lstrip(" ")) for l in lines if l.strip()]
    cut = min(indents) if indents else 0
    return [l[cut:] for l in lines]


def manifest():
    js = (LEARN / "assets" / "course.js").read_text()
    return [(int(n), slug) for n, slug in re.findall(r'\[(\d+), "([a-z0-9-]+)", "', js)]


def main():
    errors, warnings = [], []
    lessons = manifest()
    expected = {f"{n:04d}-{slug}.html" for n, slug in lessons}
    present = {p.name for p in (LEARN / "lessons").glob("*.html")}
    missing = sorted(expected - present)
    for name in sorted(present - expected):
        errors.append(f"lessons/{name}: not in the manifest")

    pages = sorted((LEARN / "lessons").glob("*.html")) + sorted((LEARN / "reference").glob("*.html"))
    for page in pages:
        rel = page.relative_to(LEARN)
        parser = Page()
        parser.feed(page.read_text())
        for attrs, text in parser.blocks:
            src = attrs.get("data-src")
            if not src:
                continue
            path = ROOT / src
            if not path.exists():
                errors.append(f"{rel}: data-src {src} does not exist")
                continue
            file_lines = [l.rstrip() for l in path.read_text().split("\n")]
            stripped = {l.strip() for l in file_lines}
            excerpt = dedent(text)
            if attrs.get("data-line"):
                n = int(attrs["data-line"])
                first = excerpt[0].strip() if excerpt else ""
                actual = file_lines[n - 1].strip() if 0 < n <= len(file_lines) else "<past end>"
                if first != actual:
                    errors.append(f"{rel}: {src}:{n} is {actual!r}, excerpt starts {first!r}")
            for line in excerpt:
                s = line.strip()
                if not s or s in ("// …", "# …", "…") or s.endswith("// …") or s.startswith("// …"):
                    continue
                if s not in stripped:
                    warnings.append(f"{rel}: line not found verbatim in {src}: {s!r}")
        if page.parent.name == "lessons":
            if not parser.has_source:
                errors.append(f"{rel}: no primary source")
            if not parser.quizzes:
                warnings.append(f"{rel}: no quiz")
            for i, options in enumerate(parser.quizzes, 1):
                if i in parser.fixed:  # fixed-order options are code or short labels
                    continue
                counts = [len(o.split()) for o in options]
                if len(set(counts)) > 1 and max(counts) - min(counts) > 1:
                    warnings.append(f"{rel}: quiz {i} option word counts differ: {counts}")

    print(f"{len(present & expected)}/{len(expected)} lessons present; {len(missing)} still to write")
    for w in warnings:
        print("warning:", w)
    for e in errors:
        print("ERROR:", e)
    return 1 if errors else 0


if __name__ == "__main__":
    sys.exit(main())
