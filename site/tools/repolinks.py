#!/usr/bin/env python3
"""mdBook preprocessor for the putio-sync docs (feature 054).

The chapters are {{#include}}s of README.md sections and of whole Markdown files,
so their links are written for GitHub. This rewrites them for the book:

- `#anchor` in a README section points at the chapter holding that heading
- `README.md#x`, `INSTALLATION.md#x`, ... point at the chapter for that file or heading
- `site/...` (screenshots) points at the site root
- any other relative path points at the file on GitHub

It also ends every chapter with the not-affiliated line (FR-005). When the
book's language isn't English, chapter text comes from the site/i18n catalog
(see docs_i18n.py) and each page starts with a machine-translation notice.
With PUTIO_SYNC_DOCS_STRINGS=<file>, an English build writes every string a
translation needs to that file (site/tools/translate-docs.py uses it).

Unresolved README anchors are reported on stderr and left as they are.
"""
import json
import os
import re
import sys

import docs_i18n

REPO = "https://github.com/devclinton/putio-sync"
AFFILIATION = "putio-sync is an independent project and is not affiliated with Put.io."
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
    lang = context["config"]["book"].get("language") or "en"
    catalog = docs_i18n.load(lang) if lang != "en" else {}
    up = "../" if lang == "en" else "../../"   # from a page to the site root
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
            return up + path[len("site/"):]
        kind = "tree" if os.path.isdir(os.path.join(ROOT, path)) else "blob"
        return f"{REPO}/{kind}/main/{path}{hash_frag}"

    wanted = set()
    for c in chs:
        english = c["path"] in docs_i18n.ENGLISH_ONLY
        content = c["content"].rstrip() + f"\n\n---\n\n*{AFFILIATION}*\n"
        if not english:
            wanted.update(docs_i18n.strings(content))
            wanted.add(docs_i18n.protect(c["name"])[0])
        if catalog and not english:
            content = docs_i18n.translate_content(content, catalog, slug)
            c["name"] = docs_i18n.translate(c["name"], catalog)
        out = []
        for line, fenced in outside_fences(content):
            if fenced or line.startswith("[![") or "](" not in line:
                out.append("" if line.startswith("[![") and not fenced else line)
                continue
            out.append(LINK.sub(lambda m: m.group(1) + rewrite(m.group(2), c) + m.group(3), line))
        c["content"] = "\n".join(out)
        if catalog and not english:
            english_page = "../" + c["path"][:-3] + ".html"
            note = docs_i18n.translate(docs_i18n.NOTICE.replace("{0}", f"({english_page})"), catalog)
            c["content"] = f"> {note}\n\n" + c["content"]
        del c["_src"]
    if lang == "en" and os.environ.get("PUTIO_SYNC_DOCS_STRINGS"):
        wanted.add(docs_i18n.NOTICE)
        with open(os.environ["PUTIO_SYNC_DOCS_STRINGS"], "w") as f:
            json.dump({docs_i18n.key(s): s for s in sorted(wanted)}, f, ensure_ascii=False, indent=1)
    json.dump(book, sys.stdout)


if __name__ == "__main__":
    main()
