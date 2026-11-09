# Feature Specification: Home-Server App Store Listings

**Feature Branch**: `052-app-store-listings`

**Created**: 2026-11-09

**Status**: Draft (files ready; submissions wait for a public repository and a tagged release)

**Input**: "Distribute through Flatpak and similar stores the way it already ships on the Snap
Store … I would like the software to be used more." Flatpak was dropped (see
[research.md](research.md) R1); the maintainer chose the three channels that reach the people
who run Put.io with Radarr and Sonarr on a home server: Unraid Community Applications, the
home-lab app stores (Umbrel, CasaOS, plus custom-app hosts), and discovery lists.

## Context

- putio-sync already ships as a container image (`ghcr.io/devclinton/putio-sync`, multi-arch
  on tags), a snap, `.deb` files and Windows zips. Its users are mostly home-server owners,
  who install apps from their platform's app store rather than with `docker run`.
- The image starts in setup mode with no config, prints a one-time setup code to its log, and
  accepts `PUTIO_SYNC_API_TOKEN` in place of the code. It runs as 99:100 by default and works
  as any other user that owns the mounted folders (checked as 1000:1000).
- Every store links to the source repository, so none of this ships until the repository is
  public, has a `LICENSE`, and has a tagged release.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Install from Unraid Community Applications (Priority: P1)

An Unraid owner searches CA for putio-sync, installs it with the default paths, opens the
web UI and finishes the setup wizard.

**Why this priority**: Unraid is where the *arr crowd lives, and CA is how they install apps.

**Independent Test**: Add `packaging/unraid/putio-sync.xml` as a template on an Unraid server;
the container starts, the WebUI link opens the setup page, and the code from the log (or the
Web UI token) unlocks the wizard.

**Acceptance Scenarios**:

1. **Given** the template with defaults, **When** the container starts, **Then** `/health`
   reports `setup` and the WebUI link opens port 15432.
2. **Given** a Web UI token was set, **When** the user enters it in the wizard, **Then** the
   wizard opens.

### User Story 2 - Install from Umbrel or CasaOS (Priority: P2)

An Umbrel or CasaOS owner installs putio-sync from the app store; downloads land in the
platform's shared Downloads folder where its media apps see them.

**Independent Test**: The compose files pass `docker compose config -q`; the image runs as
1000:1000 with the same mounts and reaches setup mode; on Umbrel the app password unlocks the
wizard.

### User Story 3 - Any Docker host with a compose file (Priority: P2)

A TrueNAS SCALE, Runtipi, Portainer or Dockge user pastes one compose file as a custom app.

**Independent Test**: `docker compose -f packaging/docker-compose/compose.yaml config -q`
passes and `up` reaches setup mode with a passing health check.

### User Story 4 - People find it (Priority: P3)

Someone looking for a Put.io downloader finds putio-sync in a list or post.

**Independent Test**: The drafts (awesome-selfhosted entry, r/selfhosted and r/putio posts,
selfh.st submission, a note to Put.io) exist for the maintainer to post. Nothing is posted by
an agent.

## Requirements *(mandatory)*

- **FR-001**: Every listing says putio-sync is unofficial and not affiliated with Put.io, and
  uses the project's own icon (no Put.io logo).
- **FR-002**: Every listing maps `/config`, `/downloads` and `/watches` and the web UI port
  15432, and tells the user how to get past setup (log code or token).
- **FR-003**: Store packages pin a released version (Umbrel also pins the image digest);
  only the Unraid template and the generic compose file follow `latest`.
- **FR-004**: The repository has a `LICENSE` matching `Cargo.toml` (MIT).
- **FR-005**: `packaging/STORES.md` lists each store's submission and per-release update steps.
- **FR-006**: Docs no longer advertise channels that don't exist (Flathub) and use the real
  GitHub owner (`devclinton`).

## Success Criteria

- **SC-001**: All compose files validate; the template XML parses.
- **SC-002**: The image reaches setup mode as 1000:1000 with the store mounts.
- **SC-003**: `make ci` passes.

## Out of scope

- Flathub (R1), AUR, Nix, Scoop, AppImage (R4), Runtipi's official store (closed to new apps).
- Submitting to any store or posting anywhere: the maintainer does that.
