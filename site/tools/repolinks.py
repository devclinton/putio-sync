#!/usr/bin/env python3
"""mdBook preprocessor for the putio-sync docs (feature 054).

The chapters are {{#include}}s of README.md sections and of whole Markdown files,
so their links are written for GitHub. This rewrites them for the book:

- `#anchor` in a README section points at the chapter holding that heading
- `README.md#x`, `INSTALLATION.md#x`, ... point at the chapter for that file or heading
- `site/...` (screenshots) points at the site root
- any other relative path points at the file on GitHub

It also ends every chapter with the not-affiliated line (FR-005).

Unresolved README anchors are reported on stderr and left as they are.
"""
import json
import os
import re
import sys

REPO = "https://github.com/devclinton/putio-sync"
NOTICE = "\n\n---\n\n*putio-sync is an independent project and is not affiliated with Put.io.*\n"
BOOK = os.path.dirname(os.path.abspath(__file__)) + "/../book"
ROOT = os.path.normpath(BOOK + "/../..")
INCLUDE = re.compile(r"\{\{#include \.\./\.\./\.\./([^:}]+)(?::([^}]+))?\}\}")
LINK = re.compile(r"(\]\()([^)\s]+)((?:\s+\"[^\"]*\")?\))")
HEADING = re.compile(r"^#{1,6}\s+(.*?)\s*#*\s*$")
HTML_ID = re.compile(r"<a\s+id=\"([^\"]+)\"")


def slug(text):
    text = re.sub(r"\[([^\]]*)\]\([^)]*\)", r"\1", text).strip().lower()
    return "".join(c if c.isalnum() or c in "-_" else "-" if c == " " else "" for c in text)


def outside_fences(content):
    fence = False
    for line in content.split("\n"):
        if line.lstrip().startswith("```"):
            fence = not fence
            yield line, True
            continue
        yield line, fence


def chapters(items):
    for item in items:
        chapter = item.get("Chapter") if isinstance(item, dict) else None
        if chapter:
            yield chapter
            yield from chapters(chapter.get("sub_items", []))


def source_of(path):
    """(repository file, anchor) the chapter file includes."""
    with open(os.path.join(BOOK, "src", path)) as f:
        m = INCLUDE.search(f.read())
    return (m.group(1), m.group(2)) if m else (None, None)


def main():
    if len(sys.argv) > 1 and sys.argv[1] == "supports":
        sys.exit(0)
    context, book = json.load(sys.stdin)
    items = book.get("items", book.get("sections", []))
    chs = [c for c in chapters(items) if c.get("path")]

    whole = {}      # INSTALLATION.md -> installation.md
    owner = {}      # README heading slug -> chapter path
    for c in chs:
        src, anchor = source_of(c["path"])
        c["_src"] = src
        if src and not anchor:
            whole[src] = c["path"]
        if src == "README.md":
            for line, fenced in outside_fences(c["content"]):
                if fenced:
                    continue
                m = HEADING.match(line)
                for s in [slug(m.group(1))] if m else HTML_ID.findall(line):
                    owner.setdefault(s, c["path"])

    def rewrite(target, chapter):
        if re.match(r"^[a-z]+:", target):
            return target
        path, _, frag = target.partition("#")
        if not path:
            if chapter["_src"] != "README.md":
                return target
            dest = owner.get(frag)
            if dest is None:
                print(f"repolinks: {chapter['path']}: no heading for #{frag}", file=sys.stderr)
                return target
            return target if dest == chapter["path"] else f"{dest}#{frag}"
        path = os.path.normpath(path.removeprefix("./"))
        hash_frag = f"#{frag}" if frag else ""
        if path == "README.md":
            dest = owner.get(frag) if frag else "introduction.md"
            if dest is None:
                print(f"repolinks: {chapter['path']}: no README heading #{frag}", file=sys.stderr)
                dest = "introduction.md"
            return dest + hash_frag
        if path in whole:
            return whole[path] + hash_frag
        if path.startswith("site/"):
            return "../" + path[len("site/"):]
        kind = "tree" if os.path.isdir(os.path.join(ROOT, path)) else "blob"
        return f"{REPO}/{kind}/main/{path}{hash_frag}"

    for c in chs:
        out = []
        for line, fenced in outside_fences(c["content"]):
            if fenced or line.startswith("[![") or "](" not in line:
                out.append("" if line.startswith("[![") and not fenced else line)
                continue
            out.append(LINK.sub(lambda m: m.group(1) + rewrite(m.group(2), c) + m.group(3), line))
        c["content"] = "\n".join(out).rstrip() + NOTICE
        del c["_src"]
    json.dump(book, sys.stdout)


if __name__ == "__main__":
    main()
