#!/usr/bin/env python3
"""Fill the docs catalogs in site/i18n with atranslate-cli (feature 054, US4).

Builds the English book once to collect every string the docs need, sends only
the strings each catalog lacks to the atranslate service, and drops strings the
docs no longer have. Needs mdbook, `atranslate-cli` on PATH and ATRANSLATE_URL
set to the translation service (ATRANSLATE_TOKEN too, if it wants one).

  site/tools/translate-docs.py          every language in docs_i18n.LANGUAGES
  site/tools/translate-docs.py de,fr    just these
"""
import json
import os
import subprocess
import sys
import tempfile

import docs_i18n

ROOT = os.path.normpath(os.path.dirname(os.path.abspath(__file__)) + "/../..")


def write(path, catalog):
    with open(path, "w") as f:
        json.dump(catalog, f, ensure_ascii=False, indent=0, sort_keys=True)
        f.write("\n")


def main():
    langs = sys.argv[1].split(",") if len(sys.argv) > 1 else docs_i18n.LANGUAGES
    server = os.environ.get("ATRANSLATE_URL")
    if not server:
        sys.exit("Set ATRANSLATE_URL to the atranslate service, e.g. http://<host>:8400")
    os.makedirs(docs_i18n.I18N, exist_ok=True)

    with tempfile.TemporaryDirectory(prefix="putio-sync-docs-i18n-") as tmp:
        strings_file = os.path.join(tmp, "strings.json")
        env = dict(os.environ, PUTIO_SYNC_DOCS_STRINGS=strings_file, MDBOOK_BUILD__BUILD_DIR=os.path.join(tmp, "book"))
        subprocess.run(["mdbook", "build", os.path.join(ROOT, "site/book")], env=env, check=True, stdout=subprocess.DEVNULL)
        with open(strings_file) as f:
            english = json.load(f)

        def usable(have, k):
            return isinstance(have.get(k), str) and docs_i18n.repair(have[k], english[k]) is not None

        missing, todo = {}, []
        for lang in langs:
            have = docs_i18n.load(lang)
            gaps = [k for k in english if not usable(have, k)]
            if gaps:
                todo.append(lang)
                missing.update((k, english[k]) for k in gaps)

        if todo:
            words = sum(len(s.split()) for s in missing.values())
            print(f"{len(missing)} strings ({words} words) to {','.join(todo)}")
            write(os.path.join(tmp, "en.json"), missing)
            args = ["atranslate-cli", "json", os.path.join(tmp, "en.json"),
                    "--to", ",".join(docs_i18n.SERVICE.get(l, l) for l in todo),
                    "--out-dir", tmp, "--timeout", os.environ.get("ATRANSLATE_TIMEOUT", "43200"),
                    "--server", server, "--quiet"]
            if os.environ.get("ATRANSLATE_TOKEN"):
                args += ["--token", os.environ["ATRANSLATE_TOKEN"]]
            if subprocess.run(args).returncode:
                print("atranslate-cli failed; keeping what it wrote", file=sys.stderr)

        for lang in langs:
            have = docs_i18n.load(lang)
            fresh_path = os.path.join(tmp, f"{docs_i18n.SERVICE.get(lang, lang)}.json")
            fresh = json.load(open(fresh_path)) if os.path.exists(fresh_path) else {}
            merged = {}
            for k, en in english.items():
                for source in (fresh, have):
                    t = source.get(k)
                    if isinstance(t, str) and docs_i18n.repair(t, en) is not None:
                        merged[k] = t
                        break
            print(f"{lang}: {len(merged)} of {len(english)} strings")
            # No catalog means no book for that language (build.sh)
            if merged:
                write(os.path.join(docs_i18n.I18N, f"{lang}.json"), merged)


if __name__ == "__main__":
    main()
