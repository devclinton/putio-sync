# Set up on a Raspberry Pi

> **Not yet tested on real hardware.** These steps follow the arm64 builds, but nobody has
> walked through them on a Pi yet. If a step doesn't match what you see, please
> [open an issue](https://github.com/devclinton/putio-sync/issues).

**You'll need:**
- a Raspberry Pi 4 or 5 (or another arm64 board) running a **64-bit** OS, such as
  Raspberry Pi OS (64-bit);
- storage for the downloads, ideally a USB disk rather than the SD card;
- a Put.io account.

**You'll end with:** putio-sync running at every boot, downloading your Put.io files to the
disk.

## 1. Check the OS is 64-bit

```bash
uname -m
```

It must print `aarch64`. If it prints `armv7l`, the OS is 32-bit and putio-sync won't run.
Reinstall with the 64-bit Raspberry Pi OS from Raspberry Pi Imager.

## 2. Mount the download disk

Mount the USB disk at a fixed path, such as `/mnt/media`, with an `/etc/fstab` entry so it
comes back after a reboot. Then make a folder on it that your user owns:

```bash
sudo mkdir -p /mnt/media/putio
sudo chown "$USER": /mnt/media/putio
```

## 3. Install putio-sync

Choose one of these.

**Docker** (if you already use Docker on the Pi): follow the
[Docker and Compose guide](docker.md), with `/mnt/media/putio` as the downloads folder.
The image has an arm64 build. Then skip to step 5.

**The .deb package** (Raspberry Pi OS is Debian-based):

```bash
curl -LO https://github.com/devclinton/putio-sync/releases/latest/download/putio-sync_VERSION_arm64.deb
sudo dpkg -i putio-sync_VERSION_arm64.deb
putio-sync --version
```

Replace `VERSION` with the latest version from the
[releases page](https://github.com/devclinton/putio-sync/releases/latest).

## 4. Run it at boot

Create `/etc/systemd/system/putio-sync.service`, with your user name in place of `pi`:

```ini
[Unit]
Description=putio-sync
Wants=network-online.target
After=network-online.target

[Service]
User=pi
ExecStart=/usr/bin/putio-sync daemon
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

## 5. Run the setup wizard

From another computer, open `http://<pi-address>:15432/` and follow
[your first sync](first-sync.md). Use `/mnt/media/putio` as the download folder (or
`/downloads` with Docker).

## Keep it updated

putio-sync shows new releases on its **What's new** page. With the .deb, download and
install the new one with `dpkg -i` as in step 3, then `sudo systemctl restart putio-sync`.
With Docker, see [Docker and Compose](docker.md).

## Next

- A Pi is a good home for the media server. See [Media server](../media-server.md).
- [Linux](linux.md) covers the standalone binary, if you'd rather not use the .deb.
