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


def main():
    langs = sys.argv[1].split(",") if len(sys.argv) > 1 else docs_i18n.LANGUAGES
    if not os.environ.get("ATRANSLATE_URL"):
        sys.exit("Set ATRANSLATE_URL to the atranslate service, e.g. http://<host>:8400")

    with tempfile.TemporaryDirectory(prefix="putio-sync-docs-i18n-") as tmp:
        strings_file = os.path.join(tmp, "strings.json")
        env = dict(os.environ, PUTIO_SYNC_DOCS_STRINGS=strings_file, MDBOOK_BUILD__BUILD_DIR=os.path.join(tmp, "book"))
        subprocess.run(["mdbook", "build", os.path.join(ROOT, "site/book")], env=env, check=True, stdout=subprocess.DEVNULL)
        with open(strings_file) as f:
            english = json.load(f)
    docs_i18n.fill(english, langs)


if __name__ == "__main__":
    main()
