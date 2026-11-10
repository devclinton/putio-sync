# Research: 054 Project Website

## R1 Site generator

**Decision**: mdBook for the docs, a hand-written static page for the landing page, both
assembled into one Pages artifact by the workflow.

**Rationale**: mdBook is a single Rust binary (the project already uses Rust), renders
CommonMark with a sidebar, built-in client-side search, light and dark themes, and
`{{#include ../README.md:anchor}}` so chapters can be cut out of the existing files without
copying them. It loads no third-party assets by default once the bundled fonts are kept
local. A landing page is about one screen of content and wants full control of the layout,
which no docs theme gives.

**Alternatives considered**:

- **Jekyll (Pages' built-in)**: no Actions build needed, but the theme ecosystem is aging,
  it needs Ruby locally to preview, and it renders the README as one page, which is the
  problem we have.
- **Zola**: one Rust binary and good for landing pages, but docs navigation and search need
  a third-party theme; two tools' worth of templating for one site.
- **Astro Starlight / Docusaurus / MkDocs Material**: polished, but add a Node or Python
  toolchain and dependency updates to a Rust repo for a small site.

## R2 Splitting the README into pages

**Decision**: add HTML comment anchors (`<!-- ANCHOR: first-run -->` … `<!-- ANCHOR_END -->`)
around each top-level section of the manual in README.md; each docs chapter is a one-line
`{{#include}}` of an anchor. INSTALLATION, PLUGINS, EVENTS and CHANGELOG are included whole.

**Rationale**: comments are invisible on GitHub, so README stays readable there, and moving
a section moves the docs page with it. Relative links between the Markdown files are
rewritten by a small preprocessor step (`README.md#x` → the chapter page).

**Open**: whether the README eventually shrinks to the overview and links to the site for
the manual. Decide after the site is live; the anchors work either way.

## R3 Deploy

**Decision**: `.github/workflows/pages.yml` with `actions/upload-pages-artifact` and
`actions/deploy-pages`, `permissions: pages: write, id-token: write`, and the
`github-pages` environment. Triggers per FR-002. The release workflow does not call it; the
tag trigger covers releases. Cache the mdBook binary download by version.

**Repository setting** (maintainer, once): Settings › Pages › Source: GitHub Actions.

## R4 Stable links

Store listings and the README link to page-level URLs plus a short list of anchors
(`installation.html#docker`, `#unraid`, `#snap`, `#debian`, `#macos`, `#windows`). The
link check step (lychee, offline mode over the built site) fails the build when any of
these disappear.

## R5 Screenshots

**Decision**: a Playwright script (`site/tools/screenshots.mjs`) that starts the daemon with
a temporary config in setup-complete state, points it at the fake Put.io server the
integration tests already use, seeds transfers, watches and history with made-up names,
and captures each page in light and dark at 1280×800 and 390×844. Output as WebP plus a PNG
fallback for the stores that need PNG.

**Open**: whether the fake Put.io server can run as a standalone binary or needs a small
`--demo` test harness. Check the API tests' fixtures first.

## R6 Translations (P3)

Reuse the 051 approach: machine-translated strings, reviewed terms (Radarr, Sonarr, magnet,
torrent kept as is), marked as machine-translated in the footer. The snap description's five
blocks are a starting point for the highlights.

## R7 Privacy

No analytics. GitHub Pages itself logs visitor IPs (GitHub's privacy statement applies);
the footer links to it. No cookies, no external fonts, no embedded videos.
