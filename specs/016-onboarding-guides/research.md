# Research: Onboarding guides (016)

## R1 Where the guides live

- **Decision**: new chapters in the mdBook at `site/book/src/guides/`, listed in
  `SUMMARY.md` as a "Guides" part right after the Introduction. They are written there
  directly, not included from README.md.
- **Rationale**: the roadmap's `docs/` means the docs site from 054. Most existing chapters
  are `{{#include}}`s of README.md or INSTALLATION.md because they are reference material
  kept in one place. Walkthroughs have no other home, so they are chapters of their own.
  The UI's `docs(page, id, text)` helper already builds
  `https://devclinton.github.io/putio-sync/docs/<page>.html`, so `docs("guides/unraid")`
  works as it is.
- **Alternatives**: a `docs/` folder rendered by GitHub. Rejected: it isn't translated,
  link-checked or published with the site. Sections in README.md were also rejected: the
  README is already 3000 lines of reference.

## R2 Does put.io take NZBs?

- **Decision**: no. The SABnzbd/NZBGet guide is "keep it for usenet, add putio-sync for
  torrents". It covers giving Radarr/Sonarr both clients, with protocol priority, and
  splitting categories.
- **Rationale**: put.io's help center lists magnet links, torrent files and plain URLs (plus
  video-site URLs) as what a transfer accepts. Nothing mentions usenet. Feature 011's
  research also notes SABnzbd as "the wrong protocol for torrents".
- **Sources**: [put.io](https://put.io/),
  [How do I make put.io fetch a torrent](http://help.put.io/en/articles/726124-how-do-i-make-put-io-fetch-a-torrent-for-me-right-now),
  [Can put.io download from video sites](http://help.put.io/en/articles/726189-can-put-io-download-from-video-sites-like-youtube).

## R3 Knowing the platform in the setup wizard

- **Decision**: the wizard reads `GET /update` and uses its `channel.kind`. The map from
  kind to guide is in [contracts/ui-links.md](contracts/ui-links.md). For TrueNAS, the app's
  compose template sets `PUTIO_SYNC_INSTALL=truenas`, and `Channel` gets a `Truenas`
  variant. Its mode is `instructions`, like Unraid, and its steps say to update the app from
  TrueNAS Apps.
- **Rationale**: the update checker (055) already detects the install channel by the same
  rules, and the settings and news pages already read `/update`. No new endpoint is needed.
  Without the marker, TrueNAS reads as `container` and would get the Docker guide.
- **Alternatives**: a new `/system/platform` endpoint. Rejected (Principle VII): it would
  duplicate the channel detection.
- **Open point**: if `/update` fails or the checker is off, the wizard links the index.

## R4 The UI's list of guides

- **Decision**: one small module, `ui/guides.js`, exports the guide list (slug, title, kind,
  search words) and the platform map. The palette, the wizard and the settings links all
  read from it.
- **Rationale**: there are about 18 guides and they change rarely. A static list keeps the
  UI self-contained, and lets a test compare it against the book.
- **Alternatives**: generate the list from `SUMMARY.md` at build time. Rejected: the UI has
  no build step.

## R5 Checking UI links against the book

- **Decision**: a node test, `tests/ui/guides.test.mjs`, run by `tests/ui_js.rs` like the
  others. It checks that every slug in `ui/guides.js` has a file under
  `site/book/src/guides/` and an entry in `SUMMARY.md`, and that every guide in `SUMMARY.md`
  is in the list. It also scans the UI sources for `docs("guides/…")` calls and checks
  their slugs.
- **Rationale**: lychee checks the built site, not the UI's outbound links. This test runs
  in `make ci` (Principle II).

## R6 Translation

- **Decision**: each guide PR runs `site/tools/translate-docs.py` (atranslate on localhost
  in this setup) and commits the changed `site/i18n/*.json`. UI strings go through
  `scripts/i18n-extract.mjs` and `scripts/i18n-translate.mjs` in the UI-links PR.
- **Rationale**: this matches 054 and 061. Untranslated blocks fall back to English, so a
  missed run is not broken, just English.

## R7 Raspberry Pi facts

- **Decision**: the Pi guide needs a 64-bit OS (Raspberry Pi OS 64-bit, arm64). It offers
  Docker (`ghcr.io/devclinton/putio-sync`, with arm64 on release tags) or the
  `aarch64` static musl tarball from the release. The page carries a "not yet tested on real
  hardware" note.
- **Rationale**: `release-binaries.yml` builds Linux aarch64 musl tarballs, and
  `docker.yml` pushes `linux/arm64` on tags and manual runs. No 32-bit ARM build exists.

## R8 Who walks each guide through

- **Decision**: as in design.md. Claude checks macOS (this Mac), Unraid (Mnemosyne) and
  Docker/Compose. The maintainer checks Windows, and Linux and TrueNAS SCALE in VMs. The
  Raspberry Pi guide stays marked untested. Each PR body carries a step checklist with who
  checked it.

## R9 The untested note

- **Decision**: a blockquote at the top of the page:
  `> **Not yet tested on real hardware.** …` with a link to report problems. It is removed
  once the guide is walked through.
- **Rationale**: plain mdBook markdown that translates like any other block. No
  preprocessor change is needed.
