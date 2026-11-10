# Implementation Plan: 054 Project Website

**Spec**: [spec.md](spec.md) · **Research**: [research.md](research.md)

## Summary

A static landing page plus an mdBook docs section generated from the existing Markdown,
built and deployed to GitHub Pages by one workflow. Screenshots come from a script against
fake data and are shared with the README and the store listings. No runtime code changes.

## Layout

```text
site/
├── index.html              # landing page (pitch, screenshots, highlights, install picker)
├── style.css               # tokens for light/dark, no external fonts
├── install.js              # optional platform filter; the page works without it
├── screenshots/            # output of tools/screenshots.mjs (committed)
├── book/
│   ├── book.toml           # mdBook config, output to ../../target/site/docs
│   └── src/
│       ├── SUMMARY.md      # chapter list
│       └── *.md            # one-line {{#include}}s of README anchors and whole files
└── tools/
    ├── screenshots.mjs     # Playwright capture (R5)
    └── build.sh            # mdBook + copy landing page + stamp version → target/site
.github/workflows/pages.yml # build, link check, axe check, deploy (R3)
```

Doc changes: ANCHOR comments in README.md (R2); README links to the site near the top;
STORES.md and the store files take the site as their website URL once it is live.

## Version stamping

`build.sh` reads the version from the tag (`GITHUB_REF_NAME`) or `cargo metadata` and
replaces `{{version}}` in `index.html` in the build output only.

## Checks

- `site/tools/build.sh` locally, then `npx serve target/site` to preview.
- CI: lychee offline link check over `target/site`, including the R4 anchors; axe-core
  over the landing page and one docs page; fail on critical findings.
- Lighthouse by hand on the live site for SC-001.

## Constitution check

- No runtime code, so no Rust tests; the checks above are the gate.
- No third-party requests from the site (FR-006).
- CHANGELOG: one Added line when the site goes live.
