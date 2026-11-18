# Set up on Linux

For a Linux server or desktop: Ubuntu, Debian, Fedora, Arch and others, on x86-64 or arm64.
On a Raspberry Pi, see [Raspberry Pi](raspberry-pi.md). If you already run Docker, the
[Docker guide](docker.md) is the quickest.

**You'll need:** a machine with systemd, `sudo`, and a Put.io account.

**You'll end with:** putio-sync running as a service that starts at boot, downloading your
Put.io files to a folder on the machine.

There are three ways to install it. The standalone binary works everywhere and uses the
setup wizard, so this guide starts with it. [Snap](#with-snap-instead) and
[the .deb package](#with-the-deb-package-instead) are below.

## 1. Install the binary

Download the tarball for your machine (`uname -m` prints `x86_64` or `aarch64`) from the
[releases page](https://github.com/devclinton/putio-sync/releases/latest), then:

```bash
tar -xzf putio-sync-X.Y.Z-linux-x86_64.tar.gz
sudo install putio-sync-X.Y.Z-linux-x86_64/putio-sync /usr/local/bin/
putio-sync --version
```

## 2. Make the download folder

Choose a folder that the user running putio-sync can write to:

```bash
sudo mkdir -p /srv/putio
sudo chown "$USER": /srv/putio
```

## 3. Run it as a service

Create `/etc/systemd/system/putio-sync.service`, with your user name in place of `alex`:

```ini
[Unit]
Description=putio-sync
Wants=network-online.target
After=network-online.target

[Service]
User=alex
ExecStart=/usr/local/bin/putio-sync daemon
Restart=on-failure

[Install]
WantedBy=multi-user.target
```

Then start it, and read the setup code from its log:

```bash
sudo systemctl daemon-reload
sudo systemctl enable --now putio-sync
journalctl -u putio-sync -n 20
```

```text
  putio-sync needs setting up: no config file
  Open http://localhost:15432/ and enter the setup code:

      ABCD-EFGH-JKLM
```

If a firewall is on, open the port, for example `sudo ufw allow 15432/tcp`.

## 4. Run the setup wizard

Open `http://<this-machine>:15432/` and follow [your first sync](first-sync.md). Use
`/srv/putio` as the download folder. Settings are saved in `~/.putiosync/config.toml` for
the service's user.

## Keep it updated

When a new release is out, putio-sync shows it on its **What's new** page. Download the
new tarball, `sudo install` it as in step 1, and run `sudo systemctl restart putio-sync`.

## With Snap instead

Snap sets up the service for you, but skips the wizard: you sign in from the command line.

```bash
sudo snap install putio-sync
sudo putio-sync login
sudo snap set putio-sync api-token="$(openssl rand -hex 24)"
sudo snap start --enable putio-sync.daemon
```

Then open `http://<this-machine>:15432/` and sign in with the api-token. Get it back with
`sudo snap get putio-sync api-token`.

- Settings are in `/var/snap/putio-sync/common/config.toml`, and downloads go to
  `/var/snap/putio-sync/common/downloads` until you change the download folder in
  **Settings**.
- To download to a disk under `/media` or `/mnt`, run
  `sudo snap connect putio-sync:removable-media`.
- Snap updates it on its own. To update now, run `sudo snap refresh putio-sync`.

## With the .deb package instead

On Debian and Ubuntu, the .deb installs `/usr/bin/putio-sync`:

```bash
sudo dpkg -i putio-sync_VERSION_amd64.deb
```

It doesn't add a service, so follow steps 2 to 4 above, with
`ExecStart=/usr/bin/putio-sync daemon` in the service file. To update, install the new .deb
the same way and restart the service.

## Next

- Radarr and Sonarr on the same machine can send torrents to putio-sync. See
  [Radarr and Sonarr](../radarr-and-sonarr.md).
- The machine can serve your downloads to TVs. See [Media server](../media-server.md).
