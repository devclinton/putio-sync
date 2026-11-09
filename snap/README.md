# Snap package

`snapcraft.yaml` builds putio-sync as a strictly confined core22 snap for amd64
and arm64. It ships two apps:

| App | What it runs |
|-----|--------------|
| `putio-sync` | The CLI (`local/cli-wrapper`) |
| `putio-sync.daemon` | `putio-sync daemon` as a system service (`local/daemon-wrapper`), off until enabled |

Both use one config, `/var/snap/putio-sync/common/config.toml`
(`$SNAP_COMMON`). The first run as root writes it from `config.example.toml`,
mode 600, with downloads going to `/var/snap/putio-sync/common/downloads`
(`local/ensure-config`). Run the CLI with `sudo` so it reads and writes that
file.

## Installing and setting up

```bash
sudo snap install putio-sync --edge
sudo snap set putio-sync api-token="$(openssl rand -hex 24)"
sudo snap start --enable putio-sync.daemon
```

Until Put.io is connected the daemon waits in setup mode: open
`http://<host>:15432/` and unlock the setup wizard with the api-token
(`sudo snap get putio-sync api-token` shows it) or the setup code in
`sudo snap logs putio-sync.daemon`. Or run `sudo putio-sync setup` in a
terminal. Afterwards sign in with the api-token. Metrics are on port 9742.

- **api-token**: the web UI and API token (`PUTIO_SYNC_API_TOKEN`). Setting it
  restarts a running daemon (`hooks/configure`).
- **Downloading to /media or /mnt**: `sudo snap connect putio-sync:removable-media`,
  then set `target_directory` in the config and `sudo snap restart putio-sync.daemon`.
- **Logs**: `sudo snap logs -f putio-sync.daemon`.
- **Status**: `snap services putio-sync`.

## Channels and CI

`.github/workflows/snap.yml` builds the snap, installs it, starts the daemon
and checks `/health` and the api-token. It publishes when the `SNAPCRAFT_TOKEN`
repository secret is set:

| Trigger | Channel | Version |
|---------|---------|---------|
| GitHub release | stable | The release tag, without the `v` |
| Run workflow | chosen, edge by default | The given version, or Cargo version plus commit, e.g. `0.1.1+git1a2b3c4` |

Without the secret the snap is still built and tested, and attached to the run
as the `snap-package` artifact. Pushes to main no longer build a snap; update
edge with `gh workflow run snap.yml -f channel=edge`.

### One-time Snap Store setup

```bash
sudo snap install snapcraft --classic
snapcraft login
snapcraft register putio-sync
snapcraft export-login --snaps putio-sync --acls package_access,package_push,package_update,package_release -
```

Add the exported credentials as the `SNAPCRAFT_TOKEN` repository secret
(Settings → Secrets and variables → Actions).

## Building locally

On Ubuntu with snapcraft and LXD:

```bash
snapcraft
sudo snap install --dangerous putio-sync_*.snap
```

`snapcraft clean` removes the build containers.
