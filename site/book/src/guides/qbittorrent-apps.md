# Use a qBittorrent phone app

**You'll need:** putio-sync set up and syncing ([your first sync](first-sync.md)), and the
download client turned on with a password: **Settings › Integrations › Radarr / Sonarr ›
Download client**. The same page shows the address to use under **Connection**.

**You'll end with:** a phone app that controls qBittorrent, such as nzb360, showing
putio-sync's items, with add, pause, resume and remove.

putio-sync answers as **qBittorrent**. This has been built from qBittorrent's published
API, not tested against each app, so an app may show a screen that stays empty. LunaSea
has no torrent client section, so there is nothing to connect there.

## 1. Reach putio-sync from the phone

The phone must reach putio-sync's port (`15432`). At home, use the machine's LAN address.
Away from home, use [Tailscale](../tailscale.md) rather than opening the port to the
internet.

## 2. Add it in the app

In nzb360, add a torrent client in its settings, choose **qBittorrent**, and enter:

- Address: `http://<putio-sync host>:15432`, with nothing after the port.
- Username: `putio-sync`. Password: the download client password from putio-sync.

Other qBittorrent remote apps ask for the same three things.

## What you'll see

- Each item is a Put.io transfer and its download. It shows as downloading until the files
  are on disk, then as paused (finished). Nothing shows as seeding.
- Upload speed, ratio, peers and seeds are always 0, and there are no tags.
- **Pause** stops the download to this machine; Put.io keeps going.
- **Remove** cancels the transfer; **Remove with data** also deletes the local files.
- "Remove all", speed limits, priorities and renaming do nothing or are refused.

## If something goes wrong

- **The app says the login failed:** check the user name and password. After 5 wrong tries
  the address is refused for a minute.
- **The app can't connect:** try the address in the phone's browser:
  `http://<host>:15432/api/v2/app/version` should answer `Forbidden`, which means putio-sync
  is there and the client is on. A "not found" page means the download client is off.

More: [what the qBittorrent API answers](../radarr-and-sonarr.md#the-qbittorrent-api).
