# Home-server app stores

Listings for the app stores that home-server owners browse. Each one installs the
release image `ghcr.io/devclinton/putio-sync`, so nothing here builds anything.

| Store | Files | Status |
|-------|-------|--------|
| Unraid Community Applications | [`unraid/putio-sync.xml`](unraid/putio-sync.xml) | Ready to submit |
| Umbrel App Store | [`umbrel/putio-sync/`](umbrel/putio-sync/) | Needs a release digest |
| CasaOS / ZimaOS App Store | [`casaos/putio-sync/`](casaos/putio-sync/) | Ready to submit after the first release |
| Runtipi, TrueNAS SCALE, Portainer, Dockge | [`docker-compose/compose.yaml`](docker-compose/compose.yaml) | Custom app; Runtipi's official store takes no new apps |

## Before any submission

1. The repository and the ghcr package are public, and the repository has a `LICENSE`.
2. A tagged release exists (`v0.1.1` or later), so the image has a version tag and an
   arm64 build. Pushes to main build amd64 only.
3. Two-factor authentication is on for the GitHub account (Unraid requires it).
4. Every listing says putio-sync is unofficial and not affiliated with Put.io, and the
   icon has no Put.io branding. Keep it that way.

On each release, bump the version in the Umbrel and CasaOS files (and the Umbrel digest)
and open an update PR in their store repositories.

## Unraid Community Applications

1. Load the template once on an Unraid server: Docker › Add Container › Template, paste
   the raw URL of `unraid/putio-sync.xml`, check the fields, and **Apply**. CA only
   supports XML as the Docker tab saves it, so copy the saved file from
   `/boot/config/plugins/dockerMan/templates-user/my-putio-sync.xml` back over
   `unraid/putio-sync.xml` (drop the `my-` fields such as `DateInstalled`) if it differs.
2. CA scans whole repositories for templates. Either submit this repository, or keep a
   small `devclinton/unraid-templates` repository with the XML and a `ca_profile.xml`.
3. Submit the repository through the form linked from the "CA Application Policies"
   thread on the Unraid forum, and open a support thread in the forum's Docker
   Containers board. Approval takes up to 48 hours.

## Umbrel

1. After the release, get the multi-arch digest and replace
   `REPLACE_WITH_RELEASE_DIGEST`:
   `docker buildx imagetools inspect ghcr.io/devclinton/putio-sync:0.1.1`
2. Fork `getumbrel/umbrel-apps`, copy `umbrel/putio-sync/` to the repository root, run
   `npm run lint:apps -- putio-sync --check-images`, and test on umbrelOS (amd64 and
   arm64 if you can).
3. Open a PR titled "Add putio-sync" using their template, with screenshots and the
   icon URL; Umbrel makes the gallery images. Set `submission` to the PR URL.
   `port: 15432` must not clash with another app; the linter checks it.

Umbrel's data folders belong to 1000:1000, so the container runs as that user. The
app password (`APP_PASSWORD`) is the web UI token, and the setup wizard accepts it.

## CasaOS / ZimaOS

1. Fork `IceWhaleTech/CasaOS-AppStore`, copy `casaos/putio-sync/` to
   `Apps/putio-sync/`, and add `icon.png` (from `snap/gui/putio-sync.png`) and one or
   two screenshots.
2. Run `docker compose config -q` and `./scripts/build_dist.sh`, then open a PR saying
   it is a new app, what it does, and how you tested it.

## Custom apps (Runtipi, TrueNAS SCALE, Portainer, Dockge)

Point users at `docker-compose/compose.yaml`; INSTALLATION.md has the steps.
