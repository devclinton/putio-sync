# Implementation Plan: 052 Home-Server App Store Listings

**Spec**: [spec.md](spec.md) · **Research**: [research.md](research.md)

## Summary

Packaging files only; no code changes. Each store gets its own folder under `packaging/`,
all pointing at the release image. `packaging/STORES.md` holds the submission and release
steps. A root `LICENSE` and doc fixes make the repository ready to be linked from stores.

## Layout

```text
LICENSE                                  # MIT, matching Cargo.toml
packaging/
├── STORES.md                            # per-store status, submission and update steps
├── docker-compose/compose.yaml          # generic; Runtipi/TrueNAS/Portainer/Dockge custom app
├── unraid/putio-sync.xml                # CA template (v2)
├── umbrel/putio-sync/                   # umbrel-app.yml, docker-compose.yml, data/*/.gitkeep
└── casaos/putio-sync/docker-compose.yml # x-casaos compose
```

Doc changes: README and INSTALLATION drop Flathub and point at the stores; README's outdated
Docker Compose example is replaced by the generic file; `devclinton` → `devclinton`
everywhere (workflows, Cargo.toml, Homebrew/winget/Debian files); ROADMAP drops Flathub.

## Constitution check

- No runtime code, so no new tests; validation is `docker compose config -q`, an XML parse,
  and a container start as 1000:1000 (research R5).
- CHANGELOG gets an Added line.
- Nothing is submitted or posted by an agent.

## Release-time steps (outside this feature)

1. Make the repository and the ghcr package public; tag the first release.
2. Fill in the Umbrel digest and open the store PRs (STORES.md).
