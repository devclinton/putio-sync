#!/usr/bin/env python3
"""Machine translation of the landing page, site/index.html.

The page is written once in English. Each block (title, headings, paragraphs,
list items, captions, buttons, links outside those) and each alt, aria-label
and data-copy* attribute is looked up in site/i18n/landing/<lang>.json by a
hash of its English text, the same way as the docs (docs_i18n). Tags, code,
URLs, product names and anything marked translate="no" become {0}, {1}...
placeholders, so the translator never sees them.

  site/tools/landing_i18n.py build <out-dir> <version>   index.html and <lang>/index.html
  site/tools/landing_i18n.py translate [de,fr]            fill the catalogs with atranslate-cli
"""
import html
import os
import re
import sys
from html.parser import HTMLParser

import docs_i18n

ROOT = os.path.normpath(os.path.dirname(os.path.abspath(__file__)) + "/../..")
PAGE = os.path.join(ROOT, "site/index.html")
CATALOGS = os.path.join(docs_i18n.I18N, "landing")
SITE = "https://devclinton.github.io/putio-sync/"

BLOCKS = {"title", "h1", "h2", "h3", "p", "li", "figcaption", "button"}
ATTRS = {"alt", "aria-label", "data-copy", "data-copied", "data-copy-failed"}
SKIP = {"pre", "code", "script", "style"}
VOID = {"meta", "link", "img", "source", "br", "hr", "input", "wbr"}
RTL = {"ar"}
# Each language's name in that language, for the language menu
NAMES = {"en": "English", "ar": "العربية", "de": "Deutsch", "es": "Español", "fr": "Français",
         "hi": "हिन्दी", "id": "Bahasa Indonesia", "it": "Italiano", "ja": "日本語", "ko": "한국어",
         "nl": "Nederlands", "pl": "Polski", "pt": "Português", "ru": "Русский", "th": "ไทย",
         "tr": "Türkçe", "uk": "Українська", "vi": "Tiếng Việt", "zh": "简体中文", "zh-TW": "繁體中文"}
# Added to every translated page's footer, and the language menu's hidden label
NOTICE = '<p class="notice">This page was machine-translated from English and may contain mistakes. <a href="../" hreflang="en" lang="en">Read the English version</a>.</p>'
LABEL = "Language"

PROTECT = re.compile(r'<(\w+)\b[^>]*\btranslate="no"[^>]*>.*?</\1>|<code\b[^>]*>.*?</code>|\{\{version\}\}|[\w.]*&#?\w+;\w*|'
                     + docs_i18n.PROTECT.pattern, re.S)


def protect(text):
    values = []

    def sub(m):
        values.append(m.group(0))
        return f"{{{len(values) - 1}}}"

    return PROTECT.sub(sub, text), values


class Element:
    def __init__(self, tag, attrs, start, inner, parent):
        self.tag, self.attrs, self.start, self.inner, self.parent = tag, dict(attrs), start, inner, parent
        self.inner_end = None

    def ancestors(self):
        e = self.parent
        while e:
            yield e
            e = e.parent


class Parser(HTMLParser):
    """Every element of the page with the offsets of its start tag and content."""

    def __init__(self, source):
        super().__init__(convert_charrefs=True)
        self.lines = [0]
        for line in source.splitlines(keepends=True):
            self.lines.append(self.lines[-1] + len(line))
        self.elements, self.stack = [], []
        self.feed(source)

    def here(self):
        line, col = self.getpos()
        return self.lines[line - 1] + col

    def handle_starttag(self, tag, attrs):
        start = self.here()
        e = Element(tag, attrs, start, start + len(self.get_starttag_text()), self.stack[-1] if self.stack else None)
        self.elements.append(e)
        if tag not in VOID:
            self.stack.append(e)

    def handle_endtag(self, tag):
        while self.stack:
            e = self.stack.pop()
            if e.tag == tag:
                e.inner_end = self.here()
                break


def untranslated(e):
    return any(a.tag in SKIP or a.attrs.get("translate") == "no" for a in [e, *e.ancestors()])


def segments(source):
    """(start, end, kind, text) for each translatable piece of source: kind is
    "inner" for an element's content, "title" for the page title (plain text),
    or the name of an attribute, whose (start, end) is the element's start tag."""
    for e in Parser(source).elements:
        if untranslated(e):
            continue
        block = e.tag in BLOCKS or (e.tag == "a" and not any(a.tag in BLOCKS for a in e.ancestors()))
        if block and e.inner_end is not None and not any(a.tag in BLOCKS for a in e.ancestors()):
            yield e.inner, e.inner_end, "title" if e.tag == "title" else "inner", " ".join(source[e.inner:e.inner_end].split())
        for name, value in e.attrs.items():
            if value and (name in ATTRS or (name == "content" and e.attrs.get("name") == "description")):
                yield e.start, e.inner, name, value


def wanted(protected):
    return re.search(r"[A-Za-z]{2}", docs_i18n.TOKEN.sub("", protected)) is not None


def strings(source):
    for _, _, kind, text in segments(source):
        p, _ = protect(text)
        if wanted(p):
            yield p


def lookup(text, catalog):
    """(the catalog's translation of text, still protected, or None; the placeholder values)"""
    p, values = protect(text)
    t = catalog.get(docs_i18n.key(p))
    return (docs_i18n.repair(t, p) if t else None), values


def translate(text, catalog, kind):
    t, values = lookup(text, catalog)
    if not t:
        return text
    if kind != "inner":
        return html.escape(docs_i18n.restore(t, values))
    # New text from the translator is escaped; the placeholders put back the page's own markup
    return docs_i18n.restore(html.escape(t, quote=False), values)


def translate_page(source, catalog):
    edits = {}
    for start, end, kind, text in segments(source):
        t = translate(text, catalog, kind)
        if kind == "title":
            edits[start, end] = t
        elif kind == "inner":
            if lookup(text, catalog)[0]:
                edits[start, end] = t
            elif wanted(protect(text)[0]):
                # Still English: say so, which also keeps its punctuation in place on a right-to-left page
                edits[start, end] = f'<span lang="en" dir="ltr">{text}</span>'
        elif t != html.escape(text):
            tag = edits.get((start, end), source[start:end])
            edits[start, end] = re.sub(rf'(\s{re.escape(kind)}=")[^"]*(")', lambda m: m.group(1) + t + m.group(2), tag, count=1)
    # Blocks never nest and a start tag is never inside a translated block's
    # content, so applying the edits from the end keeps the earlier offsets right
    for (start, end), text in sorted(edits.items(), reverse=True):
        source = source[:start] + text + source[end:]
    return source


def relink(source, lang, docs):
    """Relative links from a page one folder down: docs/ goes to the book in the
    same language when there is one."""
    def sub(m):
        url = m.group(2)
        if re.match(r"[a-z]+:|#|/|\./|\.\./", url):
            return m.group(0)
        if url.startswith("docs/") and lang in docs:
            url = f"docs/{lang}/" + url[len("docs/"):]
        return f'{m.group(1)}"../{url}"'

    return re.sub(r'(\s(?:href|src|srcset)=)"([^"]*)"', sub, source)


def menu(lang, langs, catalog):
    here = "./" if lang == "en" else "../"
    alternates = "".join(f'\n  <link rel="alternate" hreflang="{l}" href="{SITE}{"" if l == "en" else l + "/"}">' for l in langs)
    alternates += f'\n  <link rel="alternate" hreflang="x-default" href="{SITE}">'
    items = "".join(
        f'\n        <li><a href="{here if l == "en" else here + l + "/"}" hreflang="{l}" lang="{l}"'
        + (' aria-current="page"' if l == lang else "") + f">{NAMES.get(l, l)}</a></li>"
        for l in langs)
    picker = (f'  <details class="langs">\n      <summary><span class="sr">{translate(LABEL, catalog, "inner")}: </span>{NAMES.get(lang, lang)}</summary>'
              f'\n      <ul>{items}\n      </ul>\n    </details>\n  ')
    return alternates, picker


def render(source, lang, langs, catalog, docs):
    page = source
    if lang != "en":
        page = translate_page(page, catalog)
        page = relink(page, lang, docs)
        page = page.replace("<footer>", "<footer>\n    " + translate_page(NOTICE, catalog), 1)
        page = page.replace('<html lang="en">', f'<html lang="{lang}"' + (' dir="rtl"' if lang in RTL else "") + ">", 1)
    alternates, picker = menu(lang, langs, catalog)
    page = page.replace('  <meta name="color-scheme"', alternates.lstrip("\n") + '\n  <meta name="color-scheme"', 1)
    return page.replace("</header>", picker + "</header>", 1)


def available(directory):
    if not os.path.isdir(directory):
        return []
    return sorted(f[:-5] for f in os.listdir(directory) if f.endswith(".json"))


def build(out, version):
    with open(PAGE) as f:
        source = f.read()
    langs = ["en", *available(CATALOGS)]
    docs = set(available(docs_i18n.I18N))
    for lang in langs:
        catalog = docs_i18n.load(lang, CATALOGS) if lang != "en" else {}
        page = render(source, lang, langs, catalog, docs).replace("{{version}}", version)
        path = os.path.join(out, "" if lang == "en" else lang, "index.html")
        os.makedirs(os.path.dirname(path), exist_ok=True)
        with open(path, "w") as f:
            f.write(page)


def main():
    if len(sys.argv) == 4 and sys.argv[1] == "build":
        build(sys.argv[2], sys.argv[3])
    elif len(sys.argv) in (2, 3) and sys.argv[1] == "translate":
        with open(PAGE) as f:
            source = f.read()
        english = {}
        for p in [*strings(source), *strings(NOTICE), protect(LABEL)[0]]:
            english[docs_i18n.key(p)] = p
        langs = sys.argv[2].split(",") if len(sys.argv) == 3 else docs_i18n.LANGUAGES
        docs_i18n.fill(english, langs, CATALOGS)
    else:
        sys.exit(__doc__)


if __name__ == "__main__":
    main()
