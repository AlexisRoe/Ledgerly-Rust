#!/usr/bin/env python3
"""Builds the course in learn/ as a static site in dist/, ready for Cloudflare Pages.

dist/ is deleted and generated again on every run, from whatever learn/ contains at that moment. Nothing about the
content is listed here, so new lessons, cheat sheets, pages, folders, images or scripts are picked up without
changing this script:

- every web file under learn/ (HTML, CSS, JS, images, fonts) is published, at the same path. Not published: hidden
  files, __pycache__, target/, and Cargo projects (any folder with a Cargo.toml, such as playground/ and capstone/);
  those are code to clone, not pages to read;
- learn/assets/icons/ is published at the site root (favicon.ico, …), with a generated web manifest;
- every relative link to something that is not published (MISSION.md, the playground README, docs/adr/…) is
  rewritten to that file on GitHub, at the deployed commit;
- code captions (`data-src`) open on GitHub instead of VS Code, through a <meta name="ledgerly-source-base"> tag
  that assets/code.js reads;
- a 404 page and Cloudflare's _headers file are added.

The build fails, rather than publishing a broken site, if:

- a relative link in a published page points at a file that isn't published;
- a lesson or cheat sheet in the course manifest (assets/course.js) has no page;
- a file linked on GitHub (rewritten links and `data-src` captions) isn't committed at the deployed commit.

Usage: python3 scripts/build-learn-site.py [--ref <commit|branch>] [--out dist]

The GitHub ref defaults to $CF_PAGES_COMMIT_SHA (set by Cloudflare's Git integration), else to the current commit.
A commit is a permalink: the links show exactly the code the deployed lessons were written against. Push it first.
Only the standard library is used, so it runs unchanged in Cloudflare's build image.
"""

import argparse
import html
import json
import os
import re
import shutil
import subprocess
import sys
from pathlib import Path
from urllib.parse import quote

ROOT = Path(__file__).resolve().parent.parent
LEARN = ROOT / "learn"
ICONS = LEARN / "assets" / "icons"
MANIFEST = LEARN / "assets" / "course.js"
FALLBACK_REPO = "https://github.com/AlexisRoe/Ledgerly-Rust"

WEB_FILES = {".html", ".css", ".js", ".mjs", ".json", ".png", ".jpg", ".jpeg", ".gif", ".svg", ".webp", ".avif",
             ".ico", ".woff", ".woff2", ".txt", ".pdf"}
SKIPPED_DIRS = {"__pycache__", "target", "node_modules"}

LINK = re.compile(r'(?<![\w-])(href|src)="([^"]*)"')
DATA_SRC = re.compile(r'\bdata-src="([^"]*)"')
EXTERNAL = re.compile(r"^(?:[a-z][a-z0-9+.-]*:|//|#)", re.IGNORECASE)

# The course manifest: lessons are `[n, "slug", "title"]`, cheat sheets `ref: "x.html"` and `["x.html", "title"]`.
MANIFEST_LESSON = re.compile(r'\[\s*(\d+)\s*,\s*"([a-z0-9-]+)"\s*,')
MANIFEST_REF = re.compile(r'\bref:\s*"([^"]+\.html)"|\[\s*"([^"]+\.html)"\s*,')


def git(*args):
    return subprocess.run(["git", *args], cwd=ROOT, check=True, capture_output=True, text=True).stdout.strip()


def github_repo():
    """The repository's GitHub URL, from the `origin` remote; the hard-coded one if there is none (CI checkouts)."""
    try:
        url = git("remote", "get-url", "origin")
    except (subprocess.CalledProcessError, FileNotFoundError):
        return FALLBACK_REPO
    m = re.match(r"^(?:git@github\.com:|https://github\.com/|ssh://git@github\.com/)([^/]+/[^/]+?)(?:\.git)?/?$", url)
    return f"https://github.com/{m.group(1)}" if m else FALLBACK_REPO


def default_ref():
    sha = os.environ.get("CF_PAGES_COMMIT_SHA")
    if sha:
        return sha
    sha = git("rev-parse", "HEAD")
    if git("status", "--porcelain", "--", "learn"):
        print("warning: learn/ has uncommitted changes; GitHub links point at HEAD, not at them", file=sys.stderr)
    try:
        if not git("branch", "--remotes", "--contains", sha):
            print(f"warning: {sha[:12]} is on no remote branch; push it or its GitHub links will 404", file=sys.stderr)
    except subprocess.CalledProcessError:
        pass
    return sha


def site_name():
    """The course title: the <h1> of learn/index.html."""
    m = re.search(r"<h1[^>]*>(.*?)</h1>", (LEARN / "index.html").read_text(encoding="utf-8"), re.DOTALL)
    return html.unescape(re.sub(r"<[^>]+>", "", m.group(1))).strip() if m else "Course"


def published_files():
    """Every file under learn/ the website serves, as paths relative to learn/."""
    found = []
    for dirpath, dirnames, filenames in os.walk(LEARN):
        here = Path(dirpath)
        dirnames[:] = sorted(
            d for d in dirnames
            if not d.startswith(".") and d not in SKIPPED_DIRS and not (here / d / "Cargo.toml").exists()
            and here / d != ICONS
        )
        for name in sorted(filenames):
            if not name.startswith(".") and Path(name).suffix.lower() in WEB_FILES:
                found.append((here / name).relative_to(LEARN))
    return found


def head_tags(page, out, repo, ref):
    """The tags injected before </head>; paths are relative so the site also works from a sub-path."""
    up = "../" * (len(page.relative_to(out).parts) - 1)
    tags = [f'  <meta name="ledgerly-source-base" content="{repo}/blob/{ref}/">']
    if (out / "favicon.ico").exists():
        tags.append(f'  <link rel="icon" href="{up}favicon.ico" sizes="any">')
    for icon in sorted(out.glob("favicon-*.png")):
        size = icon.stem.removeprefix("favicon-")
        tags.append(f'  <link rel="icon" type="image/png" sizes="{size}" href="{up}{icon.name}">')
    if (out / "apple-touch-icon.png").exists():
        tags.append(f'  <link rel="apple-touch-icon" sizes="180x180" href="{up}apple-touch-icon.png">')
    tags += [
        f'  <link rel="manifest" href="{up}site.webmanifest">',
        '  <meta name="theme-color" content="#fffff8" media="(prefers-color-scheme: light)">',
        '  <meta name="theme-color" content="#1b1a17" media="(prefers-color-scheme: dark)">',
        "",
    ]
    return "\n".join(tags)


def rewrite_page(page, out, published, repo, ref, rewritten):
    """Points links to unpublished files at GitHub and adds the head tags."""
    rel = page.relative_to(out)
    source = LEARN / rel
    text = page.read_text(encoding="utf-8")

    def fix(m):
        attr, value = m.group(1), m.group(2)
        if not value or EXTERNAL.match(value) or value.startswith("/"):
            return m.group(0)
        path, _, fragment = value.partition("#")
        target = (source.parent / path).resolve()
        if LEARN in target.parents and target.relative_to(LEARN) in published:
            return m.group(0)
        if ROOT not in target.parents:
            sys.exit(f"error: learn/{rel} links to {value}, outside the repository")
        if not target.exists():
            sys.exit(f"error: learn/{rel} links to {value}, which does not exist")
        repo_path = target.relative_to(ROOT).as_posix()
        kind = "tree" if target.is_dir() else "blob"
        url = f"{repo}/{kind}/{ref}/{quote(repo_path)}" + ("#" + fragment if fragment else "")
        rewritten.append((rel.as_posix(), value, url, repo_path))
        return f'{attr}="{url}"'

    text = LINK.sub(fix, text)
    if "</head>" not in text:
        sys.exit(f"error: learn/{rel} has no </head>")
    page.write_text(text.replace("</head>", head_tags(page, out, repo, ref) + "</head>", 1), encoding="utf-8")


def write_extras(out, name):
    if ICONS.is_dir():
        for icon in sorted(ICONS.iterdir()):
            if icon.is_file() and not icon.name.startswith("."):
                shutil.copy2(icon, out / icon.name)
    icons = [
        {"src": "/" + p.name, "sizes": p.stem.rsplit("-", 1)[-1], "type": "image/png"}
        for p in sorted(out.glob("android-chrome-*.png"))
    ]
    manifest = {
        "name": name,
        "short_name": "Ledgerly Rust",
        "start_url": "/",
        "icons": icons,
        "theme_color": "#fffff8",
        "background_color": "#fffff8",
        "display": "standalone",
    }
    (out / "site.webmanifest").write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    # Without a 404.html, Cloudflare Pages serves index.html for every unknown path (SPA mode).
    if not (out / "404.html").exists():
        (out / "404.html").write_text(f"""<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>Page not found · {html.escape(name)}</title>
  <link rel="stylesheet" href="/assets/style.css">
  <link rel="icon" href="/favicon.ico" sizes="any">
</head>
<body>
<main>
  <h1>Page not found</h1>
  <p>There is no page at this address. Go back to the <a href="/">contents</a>.</p>
</main>
</body>
</html>
""", encoding="utf-8")
    (out / "_headers").write_text("""/*
  X-Content-Type-Options: nosniff
  X-Frame-Options: DENY
  Referrer-Policy: strict-origin-when-cross-origin
  Permissions-Policy: camera=(), microphone=(), geolocation=()
""", encoding="utf-8")


def broken_links(out):
    """Relative href/src values in dist/ that don't resolve to a file in dist/."""
    broken = []
    for page in sorted(out.rglob("*.html")):
        for _, value in LINK.findall(page.read_text(encoding="utf-8")):
            if not value or EXTERNAL.match(value):
                continue
            path = value.partition("#")[0]
            if path in ("", "/"):
                continue
            base = out if path.startswith("/") else page.parent
            target = (base / path.lstrip("/")).resolve()
            if not target.is_file() or out not in target.parents:
                broken.append(f"{page.relative_to(out)} -> {value}")
    return broken


def missing_manifest_pages(out):
    """Lessons and cheat sheets that course.js links to (at runtime, so the link check can't see them) but that
    have no page."""
    if not MANIFEST.exists():
        return []
    text = MANIFEST.read_text(encoding="utf-8")
    wanted = {f"lessons/{int(n):04d}-{slug}.html" for n, slug in MANIFEST_LESSON.findall(text)}
    wanted |= {f"reference/{a or b}" for a, b in MANIFEST_REF.findall(text)}
    return sorted(p for p in wanted if not (out / p).is_file())


def missing_on_github(ref, repo_paths):
    """Repository paths that GitHub would 404 on at `ref`: not committed there."""
    try:
        files = set(git("ls-tree", "-r", "--name-only", ref).splitlines())
    except (subprocess.CalledProcessError, FileNotFoundError):
        print(f"warning: {ref} is not in the local clone; GitHub links were not checked", file=sys.stderr)
        return []
    dirs = {"/".join(f.split("/")[:i]) for f in files for i in range(1, f.count("/") + 1)}
    return sorted(p for p in repo_paths if p not in files and p not in dirs)


def fail(message, items):
    sys.exit(f"error: {message}:\n  " + "\n  ".join(items))


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--ref", help="commit or branch the GitHub links point at (default: the current commit)")
    parser.add_argument("--out", default=str(ROOT / "dist"), help="output directory (default: dist/)")
    args = parser.parse_args()
    out = Path(args.out).resolve()
    if out == ROOT or out in ROOT.parents or out == LEARN or LEARN in out.parents or out in LEARN.parents:
        sys.exit(f"error: refusing to write to {out}")
    repo = github_repo()
    ref = args.ref or default_ref()
    name = site_name()

    # Start from nothing, so files deleted from learn/ disappear from the site too.
    if out.exists():
        shutil.rmtree(out)
    out.mkdir(parents=True)
    published = published_files()
    if Path("index.html") not in published:
        sys.exit("error: learn/index.html is missing")
    for rel in published:
        (out / rel).parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(LEARN / rel, out / rel)
    write_extras(out, name)

    published_set = set(published)
    pages = [out / p for p in published if p.suffix == ".html"]
    rewritten = []
    for page in pages:
        rewrite_page(page, out, published_set, repo, ref, rewritten)

    if broken := broken_links(out):
        fail("broken links in the built site", broken)
    if missing := missing_manifest_pages(out):
        fail("assets/course.js lists pages that don't exist", missing)
    repo_paths = {r[3] for r in rewritten}
    for page in pages:
        repo_paths.update(DATA_SRC.findall(page.read_text(encoding="utf-8")))
    if missing := missing_on_github(ref, repo_paths):
        fail(f"these files are linked on GitHub but not committed at {ref[:12]}; commit them first", missing)

    for page, old, new, _ in rewritten:
        print(f"  {page}: {old} -> {new}")
    shown = out.relative_to(ROOT) if ROOT in out.parents else out
    print(f"Built {name!r} into {shown}: {len(published)} files ({len(pages)} pages), "
          f"{len(rewritten)} links rewritten to GitHub, {len(repo_paths)} repository files linked at {ref[:12]}.")


if __name__ == "__main__":
    main()
