# Research: Multi-language Web UI

## R1. How strings reach the page

The UI is plain ES modules with no build step. Elements are built with `h(tag, attrs, ...children)`
in `ui/app.js`, with English literals for `text`, children, `title`, `placeholder` and `aria-label`;
about 70 places set `textContent` directly, and about 1,100 template literals interpolate values.
Toasts and the confirm dialog go through the same paths.

**Decision**: gettext style. English stays in the code and is the catalog key. A translator watches
`document.body` (one `MutationObserver`) and translates text nodes and the three attributes as they
are added or changed, keeping the English for each node so a switch can redo the page.
**Rejected**: `t("key")` at every call site. About 1,800 edits across 50 files, a second copy of
every string in the bundle (over the size cap), and every later feature would have to remember it.

## R2. Messages with values

A template such as `` `Flagged ${name} as fake` `` becomes the key `Flagged {0} as fake`. At run time
each such key is a regular expression (`^Flagged (.+?) as fake$`); the captured values go into the
translation's `{0}`, `{1}` in whatever order it uses them, and a value that is itself a catalog string
(an appearance mode, a state) is translated too. A cheap substring test on the key's longest literal
runs before each expression. A template whose value is glued to a letter (`` `${n} file${s}` ``) is
left out: its English wording depends on the value, which a placeholder can't express. Plurals are
not modelled (spec Assumptions).

## R3. Data is not text

Only whole-string matches are translated, so a file name changes only if it equals a UI string
exactly. `code`, `pre`, `textarea`, `script`, `style` and anything under `translate="no"` (the HTML
attribute meant for this) are skipped. The language names in the picker use `translate="no"`.

## R4. Size budget

`tests/ui_assets.rs` caps what the index reaches through static imports. The cap was 480 KiB with
about 1.5 KB free. `i18n.js` (choice, browser matching, names, `setLanguage`) is static; the
translator (`translate.js`) is a dynamic `import()` and the catalogs are `fetch`ed, both only for a
language other than English. An English page grows by about 4.4 KB.
**Deviation**: the cap moves from 480 KiB to 484 KiB, as features 025, 033 and 046 raised it before.
The alternative, trimming unrelated code to make room, is out of scope.

## R5. Languages

The translation service reports its installed models at `GET /languages`. Every base language
installed at the time is shipped: ar, da, de, es, fi, fr, he, hi, id, it, ja, ko, ms, nb, nl, pl, pt,
pt-PT, ru, sv, th, tr, uk, vi, zh, zh-TW. Regional variants (es-MX, fr-CA, de-CH...) are served by the
base language. `pt` is Brazilian Portuguese, `zh` simplified and `zh-TW` traditional Chinese (zh-HK,
zh-Hant also map to it). Arabic and Hebrew set `dir="rtl"`. Names come from `Intl.DisplayNames`, so
no table of names ships.

## R6. Producing catalogs

`scripts/i18n-extract.mjs` tokenises each UI module (strings, templates with nested `${}`, comments
and regular-expression literals) and keeps what reads as text: a leading capital, or several words
that aren't class names; paths, selectors, CSS, CamelCase values and constants are dropped. It writes
`ui/i18n/en.json` (each string maps to itself), and a node test fails when that file is stale.
`scripts/i18n-translate.mjs` sends only the strings each catalog lacks to `atranslate-cli json`
(server from `ATRANSLATE_URL`) and drops strings `en.json` no longer has. A translation that loses a
`{n}` is ignored at run time and doesn't count toward coverage.

## R7. Translated Snap Store listings

**Finding: not supported.** The Snap Store keeps one summary and one description per snap. snapcraft
can take them from AppStream metadata (`adopt-info`), which does carry translations, but its
extractor (`snapcraft/meta/appstream.py` in canonical/snapcraft) keeps only elements without
`xml:lang` or with `xml:lang="en"` and drops the rest. A 2023 forum request for localised store
pages (forum.snapcraft.io/t/snap-store-page-localisation-support/34437) has no feature behind it. So
nothing is wired into `snap/`. If translated listings are wanted later, AppStream metainfo for the
Flatpak/Flathub listing does support `xml:lang` translations.
