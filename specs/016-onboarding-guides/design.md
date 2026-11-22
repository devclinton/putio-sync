# 016 Onboarding guides: design

Agreed in chat on 2026-11-18. This is the source for the Spec Kit spec, plan and tasks.

## Intent

The docs site (the mdBook in `site/book`, feature 054) is reference material. Most pages
include sections of README.md and INSTALLATION.md. It says what every setting does, but not
"I have a TrueNAS box and a put.io account, what do I do?". The guides are walkthroughs:
each one starts from a situation the reader is in and ends with something working, linking
to the reference pages for details instead of copying them.

## Readers

- **New users**: a put.io account, a box at home, and nothing set up yet.
- **Existing users**: putio-sync is running and they want to add one more piece.
- **People switching** from a local torrent or usenet client: qBittorrent, Transmission,
  Deluge, SABnzbd, NZBGet.
- **People wiring it into the usual home media stack**: Radarr, Sonarr, Prowlarr, a media
  server, Grafana.

## Goals

1. A **Guides** part of the docs site with three kinds of page, written in this order (each
   kind can ship as its own PR):
   1. **Set up on…**: one page per platform, start to finish. Install, open the web UI,
      sign in to put.io, choose folders, first sync, keep it updated, where to go next.
      Platforms: Unraid, TrueNAS SCALE, Raspberry Pi, Docker and Compose (any Linux box or
      NAS), macOS (Homebrew or the installer package), Windows (installer or winget), and
      Linux (snap, .deb or the standalone binary).
   2. **Do a task**: adding to a running setup. Radarr and Sonarr (with Prowlarr feeding
      them), folder rules and presets, watch folders, the media server, metrics and
      Grafana, and troubleshooting with `doctor` and the Logs page.
   3. **Switching from…**: qBittorrent, Transmission, Deluge, then SABnzbd and NZBGet.
2. A **Guides index** page that sends each reader to the right guide: "I'm new", "I'm adding
   something", "I'm switching".
3. **Links from the web UI** to the guides, where a reader would want them:
   - The setup wizard's first screen links to the setup guide for the platform putio-sync is
     running on. The update checker already detects this (`channel.kind` from the update
     status: `unraid`, `snap`, `homebrew`, `container`, …). When nothing matches, it links
     to the guides index.
   - The Radarr/Sonarr, folder rules, watch folder, media server and metrics settings get a
     "Guide" link beside their existing docs links.
   - The Doctor and Logs pages link to the troubleshooting guide.
   - The command palette (061) gets a **Guides** group, so typing "grafana" or "unraid"
     offers the guide. It opens in a new tab like the other docs links.
4. The guides are machine-translated with the rest of the site (`site/tools/translate-docs.py`
   into the `site/i18n` catalogs) and pass the site's link and accessibility checks.

## Decided in chat

- All three kinds of guide, in the order platform setup → tasks → switching.
- Guide links in the UI are welcome. The four places above were proposed and accepted.
- Switching guides explain the change in thinking: put.io downloads the torrent and
  putio-sync brings the finished files home, so there is no local seeding or ratio. They
  also cover moving watch folders and categories, and pointing Radarr/Sonarr at putio-sync
  in place of the old client. putio-sync speaks the Transmission RPC protocol, so in the
  *arr apps this is a download client swap.
- Usenet: put.io doesn't take NZBs (to be confirmed in research). If so, the SABnzbd/NZBGet
  guide is "add putio-sync next to it for torrents" rather than "replace it".

## Non-goals

- Rewriting the reference pages. The guides link to them; the reference stays where it is.
- A guided tour or in-app help panels. That's 014, which will link to these guides.
- New install methods or packaging changes.
- Guides for CasaOS and Umbrel beyond a mention on the index. They have one-click apps and
  can get their own guide later.

## Anti-goals (it fails even if it technically works)

- Guides that repeat the reference and drift from it. A step that needs a setting's full
  explanation links to it.
- Steps that don't match what the current release does. Every guide is walked through
  against a real build before it ships, and UI wording in the guides matches the UI.
- Guides that assume the reader already knows Docker, put.io or the *arr apps' jargon
  without a one-line explanation.
- UI links that point at pages that don't exist on the published site.

## Constraints

- Pages live under `site/book/src/guides/` and are listed in `SUMMARY.md` under a new
  "Guides" part, placed right after the Introduction.
- The site build (`site/tools/build.sh`), the lychee link check and the a11y check in
  `.github/workflows/pages.yml` must pass. The workflow's `paths` filter already covers
  `site/**`.
- UI links use the existing `docs(page, id, text)` helper in `ui/app.js`. Any new UI string
  goes through the UI i18n catalogs (`scripts/i18n-extract.mjs`, then
  `scripts/i18n-translate.mjs`).
- Repo rules: a CHANGELOG `[Unreleased]` line per change, draft PRs, no Claude attribution,
  no commits during work hours.
- Facts to check against the code, not memory: arm64 builds (release binaries and the
  Docker image ship arm64 on tags; the Pi needs a 64-bit OS), the TrueNAS app in
  `packaging/truenas`, the Unraid template in `packaging/unraid`, and Compose in
  `packaging/docker-compose`.

## Who walks each guide through

Decided in chat on 2026-11-18:

- **Claude**: macOS on this Mac, and Unraid on the maintainer's NAS. Docker and Compose can
  also be checked here.
- **The maintainer**: Windows on an old Windows machine, and Linux (snap, .deb, binary) and
  TrueNAS SCALE, each in a VM.
- **Nobody yet**: Raspberry Pi. Its PR and page say "not yet tested on real hardware" until
  someone has walked it through. Its steps come from the arm64 builds.

Each guide PR carries a checklist of its steps and who checked them.

## Settled after review (Claude's calls)

- **PR order:** guide PRs first, in the agreed order; the UI links come in a last PR once
  the guides they point at are on main. A test checks that every guide slug the UI links to
  exists in `site/book/src`, since lychee only checks the site itself.
- **Wizard mapping:** a table from `channel.kind` to guide slug. `unraid` → Unraid,
  `container`/`casaos`/`umbrel` → Docker and Compose, `snap`/`deb`/`flatpak` →
  Linux, `homebrew`/`pkg` → macOS, `winget`/`setup` → Windows, `archive`/`unknown` →
  the index (`archive` gives no OS, so the index lists every platform, the Pi included). The TrueNAS app gets
  `PUTIO_SYNC_INSTALL: truenas` and a `truenas` channel kind (updates by instructions, like
  Unraid), so its readers land on the TrueNAS guide.
- **Linux is one page** with a section each for snap, .deb and the standalone binary. Docker
  and Compose is also one page, and it covers other NAS boxes that run containers.
- **Usenet:** research confirms whether put.io accepts NZBs before that guide is written. The
  default framing is "add next to SABnzbd/NZBGet for torrents".
- **Translation:** each guide PR runs `site/tools/translate-docs.py` and commits the updated
  catalogs, as earlier docs changes did.
- **Palette:** guide entries are matched on the title in the UI language plus the English
  title and a few English keywords, the same way 061 matches pages.

## Left to the builder

- Exact page names and slugs, and how much each platform guide shares through a common
  "after install" page versus repeating a few steps.
- Whether a guide uses screenshots (`site/tools/screenshots.mjs` exists) or text alone.
- How the palette gets its guide list: a static list in the UI or a generated file.
- What TrueNAS detection reports. The TrueNAS app sets no `PUTIO_SYNC_INSTALL`, so it shows
  as `container` and the wizard would link the Docker guide. Adding `PUTIO_SYNC_INSTALL:
  truenas` plus a channel kind is a small, optional extra.
- Whether UI guide links point at the reader's UI language version of the docs (the
  `docs()` helper links the English site today).
