# Research: 052 Home-Server App Store Listings

Checked 2026-11-09 against each store's current docs and repository.

## R1. Flathub: not pursued

- **Decision**: no Flatpak. The old scaffolding (`com.putio.PutioSync.yaml`,
  `packaging/flatpak/`, `.github/workflows/flatpak.yml`) was never published and cannot be
  submitted as it is.
- **Why**: Flathub's requirements rule it out on several counts at once:
  - Generative-AI policy: "Flathub manifests must not contain AI-generated or AI-assisted
    content", AI-generated material elsewhere must be disclosed and may be rejected, and AI
    agents must not open or write submission PRs. Most of putio-sync is agent-written.
  - Console software and thin web wrappers are not accepted; putio-sync is a daemon whose UI
    is a local web page.
  - An unofficial client cannot use the vendor's domain as its ID (`com.putio.*`) or its name
    in the app name or icon.
- **Alternative**: a self-hosted `.flatpak` bundle on releases. Rejected by the maintainer
  ("skip flatpak"); the snap already covers desktop Linux.

## R2. Which stores reach the users

- **Decision**: Unraid CA, Umbrel, CasaOS/ZimaOS, and one compose file for custom-app hosts.
- **Why**: the users run Put.io with Radarr/Sonarr on a home server. These stores are how
  those platforms install apps, and all reuse the existing release image.
- Runtipi's official store says "no new applications will be accepted"; its users add a
  custom app from the compose file. TrueNAS SCALE's catalog format is heavy and changes
  often; its "Install via YAML" custom app takes the same compose file.

## R3. Store rules that shape the files

- **Unraid CA**: template XML (`Container version="2"`) in a GitHub repository; the only
  supported XML is what the Docker tab saves, so the template is loaded and re-saved once on a
  server before submission. Needs a Project or Support URL, 2FA on GitHub, open-source code,
  no extra commands after `docker run`. Submission is a form linked from the "CA Application
  Policies" forum thread.
- **Umbrel**: `umbrel-app.yml` + `docker-compose.yml` with an `app_proxy` service; image
  multi-arch and pinned as `tag@sha256:<index digest>`, no `latest`; state under
  `${APP_DATA_DIR}/data`; data owned by 1000:1000; `deterministicPassword: true` exposes
  `${APP_PASSWORD}`, wired here to `PUTIO_SYNC_API_TOKEN`; `gallery: []` for new apps.
- **CasaOS/ZimaOS**: `Apps/<Name>/docker-compose.yml` with `x-casaos` (`id`, `main`, `index`,
  `port_map`, `icon`, `title`, `category` from a fixed list, `version`), `$PUID`/`$PGID`/`$TZ`
  provided by the platform, compose must pass `docker compose config -q`.
- None of these stores states an AI policy or a naming rule for third-party clients; the
  listings still say "unofficial, not affiliated with Put.io".

## R4. Other channels considered

- Homebrew tap, AUR `-bin`, Nix flake, Scoop, AppImage: smaller audiences for a server
  daemon, and most need Linux/macOS release tarballs that the release flow doesn't build yet.
  Deferred; the Homebrew and winget files had their GitHub owner fixed.
- Discovery (awesome-selfhosted, selfh.st, r/selfhosted, r/putio, Put.io's app list) likely
  brings more users than any package manager. awesome-selfhosted wants a first release more
  than 4 months old. Drafts are kept outside the repository for the maintainer to post.

## R5. Container user

The image runs as 99:100 (Unraid's `nobody:users`). Run as `--user 1000:1000` with bind
mounts and `PUTIO_SYNC_API_TOKEN` set, it reached setup mode (`/health` → `setup`) and the
token was accepted (`/auth/status` 200), so the Umbrel and CasaOS files override the user
instead of needing PUID/PGID support in the image.
