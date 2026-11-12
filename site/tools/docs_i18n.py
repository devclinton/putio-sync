"""Machine translation of the docs book (feature 054, US4).

The book is built once in English and once per language in LANGUAGES. Chapter
text is split into blocks (headings, paragraphs, list items, table cells); each
block's translation is looked up in site/i18n/<lang>.json by a hash of its
English text, so a block that changed or was added stays in English until the
catalog is refreshed with site/tools/translate-docs.py. Code, URLs and link
targets never reach the translator: they become {0}, {1}... placeholders.
"""
import hashlib
import json
import os
import re
import subprocess
import sys
import tempfile

I18N = os.path.normpath(os.path.dirname(os.path.abspath(__file__)) + "/../i18n")

# Languages the atranslate service has models for (GET /languages), as book
# language codes; SERVICE maps the ones it names differently
LANGUAGES = ["ar", "de", "es", "fr", "hi", "id", "it", "ja", "ko", "nl", "pl", "pt",
             "ru", "th", "tr", "uk", "vi", "zh", "zh-TW"]
SERVICE = {"ar": "ar-AE"}
# Chapters kept in English: the release history and the license text
ENGLISH_ONLY = {"changelog.md", "license.md"}

NOTICE = "This page was machine-translated from English and may contain mistakes. [Read the English version]{0}."

FENCE = re.compile(r"^\s*(```|~~~)")
HEADING = re.compile(r"^(#{1,6})\s+(.*?)\s*#*\s*$")
ITEM = re.compile(r"^(\s*(?:[-*+]|\d+[.)])\s+(?:\[[ xX]\]\s+)?)(.*)$")
QUOTE = re.compile(r"^(\s*>\s?)(.*)$")
TABLE = re.compile(r"^\s*\|.*\|\s*$")
RULE = re.compile(r"^\s*(?:\|?\s*:?-{3,}:?\s*)+\|?\s*$|^\s*([-*_])(?:\s*\1){2,}\s*$")
HTML = re.compile(r"^\s*<")
# Product and platform names, which the translator otherwise renders as words
# ("Windows" -> "Fenêtres")
NAMES = ["putio-sync", "Put.io", "Windows", "macOS", "Arch Linux", "Fedora", "RHEL", "Linux", "Docker Compose", "Docker",
         "Unraid", "TrueNAS SCALE", "TrueNAS", "Synology", "QNAP", "Portainer", "Dockge",
         "Runtipi", "Umbrel", "CasaOS", "Snap", "snapd", "Flatpak", "Homebrew", "winget",
         "Radarr", "Sonarr", "Prowlarr", "Transmission", "Kodi", "Plex", "Jellyfin", "Emby",
         "DLNA", "Tailscale", "Prometheus", "Grafana", "GitHub", "Debian", "Ubuntu", "Rust",
         "SQLite", "systemd", "launchd", "OpenSubtitles", "SmartScreen", "mdBook"]
# Kept as they are: inline code, link and image targets, autolinks, bare URLs,
# inline HTML tags, {placeholders} already in the text, and NAMES
PROTECT = re.compile(r"`+[^`]*`+|\]\([^)\s]*(?:\s+\"[^\"]*\")?\)|<https?://[^>]+>|https?://[^\s)<>\]]+|</?[a-zA-Z][^>]*>|\{[^}\s]*\}"
                     r"|(?<![\w.-])(?:" + "|".join(re.escape(n) for n in NAMES) + r")(?![\w-])")
TOKEN = re.compile(r"\{(\d+)\}")


def key(text):
    return hashlib.sha1(text.encode()).hexdigest()[:12]


def protect(text):
    """English block -> (text with {n} placeholders, the values)."""
    values = []

    def sub(m):
        v = m.group(0)
        values.append(v[1:] if v.startswith("](") else v)
        n = len(values) - 1
        return f"]{{{n}}}" if v.startswith("](") else f"{{{n}}}"

    return PROTECT.sub(sub, text), values


def repair(translated, source):
    """A translation fixed up, or None when it lost or garbled a placeholder."""
    # The service's own markers leaked ("⟦ 1⟧"), with the placeholder it lost
    # put back at the end: keep the one in place
    t = re.sub(r"⟦\s*(\d+)\s*⟧", r"{\1}", translated)
    if t != translated:
        while (m := re.search(r"\s*\{(\d+)\}\s*$", t)) and t[:m.start()].count("{" + m.group(1) + "}"):
            t = t[:m.start()]
    if "⟦" in t or "⟧" in t or "\ufffd" in t:
        return None
    t = re.sub(r"\]\s+\{(\d+)\}", r"]{\1}", t)                    # "[text] {0}"
    t = re.sub(r"\{\s*(\d+)\s*\}", r"{\1}", t)                     # "{ 0 }"
    if sorted(TOKEN.findall(t)) != sorted(TOKEN.findall(source)):
        return None
    if t.count("[") != source.count("[") or t.count("]") != source.count("]"):
        return None
    t = re.sub(r"\*\*(\S[^*]*?)\s+\*\*(?=\s|$)", r"**\1**", t)     # "**Label. ** text"
    if t.count("**") % 2:
        t = t.replace("**", "")
    if t.count("`") % 2:
        return None
    return t


def restore(text, values):
    return TOKEN.sub(lambda m: values[int(m.group(1))], text)


def blocks(content):
    """Yield (start, end, prefix, text, kind) for each translatable block, with
    line indexes into content.split("\\n"). Fenced code, HTML lines, rules and
    indented code are skipped."""
    lines = content.split("\n")
    i, fence = 0, None
    while i < len(lines):
        line = lines[i]
        m = FENCE.match(line)
        if fence:
            if m and m.group(1) == fence:
                fence = None
            i += 1
            continue
        if m:
            fence = m.group(1)
            i += 1
            continue
        if not line.strip() or HTML.match(line) or RULE.match(line) or line.startswith("    ") or line.startswith("\t"):
            i += 1
            continue
        m = HEADING.match(line)
        if m:
            yield i, i + 1, m.group(1) + " ", m.group(2), "heading"
            i += 1
            continue
        if TABLE.match(line):
            yield i, i + 1, "", line, "row"
            i += 1
            continue
        m = ITEM.match(line) or QUOTE.match(line)
        prefix, text = (m.group(1), m.group(2)) if m else ("", line.strip())
        indent = " " * len(prefix) if m else ""
        j = i + 1
        # Continuation lines: indented under a list item, or a plain paragraph
        while j < len(lines) and lines[j].strip() and not (
            HEADING.match(lines[j]) or ITEM.match(lines[j]) or FENCE.match(lines[j])
            or TABLE.match(lines[j]) or HTML.match(lines[j]) or RULE.match(lines[j])
            or (m is None and QUOTE.match(lines[j]))
        ):
            more = lines[j]
            if m is not None and QUOTE.match(line):
                q = QUOTE.match(more)
                if not q:
                    break
                more = q.group(2)
            elif m is not None and not more.startswith(indent[:2] or " "):
                break
            text += " " + more.strip()
            j += 1
        yield i, j, prefix, text, "text"
        i = j


def cells(row):
    return [c.strip() for c in row.strip().strip("|").split("|")]


def strings(content):
    """Every English string the content needs, already protected."""
    for _, _, _, text, kind in blocks(content):
        for s in (cells(text) if kind == "row" else [text]):
            p, _ = protect(s)
            if re.search(r"[A-Za-z]{2}", TOKEN.sub("", p)):
                yield p


def translate(text, catalog):
    p, values = protect(text)
    t = catalog.get(key(p))
    t = repair(t, p) if t else None
    return restore(t, values) if t else text


def translate_content(content, catalog, slug):
    """content with each block replaced by its translation where the catalog
    has one; headings keep their English id."""
    lines = content.split("\n")
    out, last = [], 0
    for start, end, prefix, text, kind in blocks(content):
        out.extend(lines[last:start])
        if kind == "heading":
            t = translate(text, catalog)
            out.append(prefix + t + (f" {{#{slug(text)}}}" if t != text else ""))
        elif kind == "row":
            out.append("| " + " | ".join(translate(c, catalog) for c in cells(text)) + " |")
        else:
            out.append(prefix + translate(text, catalog))
        last = end
    out.extend(lines[last:])
    return "\n".join(out)


def load(lang, directory=I18N):
    path = os.path.join(directory, f"{lang}.json")
    if not os.path.exists(path):
        return {}
    with open(path) as f:
        return json.load(f)


def write(path, catalog):
    with open(path, "w") as f:
        json.dump(catalog, f, ensure_ascii=False, indent=0, sort_keys=True)
        f.write("\n")


def fill(english, langs, directory=I18N):
    """Send the strings each catalog in directory lacks to atranslate-cli, and
    drop strings that are no longer in english ({key: protected text}). Needs
    ATRANSLATE_URL (and ATRANSLATE_TOKEN, if the service wants one)."""
    server = os.environ.get("ATRANSLATE_URL")
    if not server:
        sys.exit("Set ATRANSLATE_URL to the atranslate service, e.g. http://<host>:8400")
    os.makedirs(directory, exist_ok=True)

    def usable(have, k):
        return isinstance(have.get(k), str) and repair(have[k], english[k]) is not None

    with tempfile.TemporaryDirectory(prefix="putio-sync-i18n-") as tmp:
        missing, todo = {}, []
        for lang in langs:
            have = load(lang, directory)
            gaps = [k for k in english if not usable(have, k)]
            if gaps:
                todo.append(lang)
                missing.update((k, english[k]) for k in gaps)

        if todo:
            words = sum(len(s.split()) for s in missing.values())
            print(f"{len(missing)} strings ({words} words) to {','.join(todo)}")
            write(os.path.join(tmp, "en.json"), missing)
            args = ["atranslate-cli", "json", os.path.join(tmp, "en.json"),
                    "--to", ",".join(SERVICE.get(l, l) for l in todo),
                    "--out-dir", tmp, "--timeout", os.environ.get("ATRANSLATE_TIMEOUT", "43200"),
                    "--server", server, "--quiet"]
            if os.environ.get("ATRANSLATE_TOKEN"):
                args += ["--token", os.environ["ATRANSLATE_TOKEN"]]
            if subprocess.run(args).returncode:
                print("atranslate-cli failed; keeping what it wrote", file=sys.stderr)

        for lang in langs:
            have = load(lang, directory)
            fresh_path = os.path.join(tmp, f"{SERVICE.get(lang, lang)}.json")
            fresh = json.load(open(fresh_path)) if os.path.exists(fresh_path) else {}
            merged = {}
            for k, en in english.items():
                for source in (fresh, have):
                    t = source.get(k)
                    if isinstance(t, str) and repair(t, en) is not None:
                        merged[k] = t
                        break
            print(f"{lang}: {len(merged)} of {len(english)} strings")
            # No catalog means no page for that language (build.sh)
            if merged:
                write(os.path.join(directory, f"{lang}.json"), merged)
