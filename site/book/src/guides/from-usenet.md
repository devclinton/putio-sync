# Use Put.io next to SABnzbd or NZBGet

**You'll need:** putio-sync set up and syncing ([your first sync](first-sync.md)), and Radarr
or Sonarr running with SABnzbd or NZBGet as a download client.

**You'll end with:** usenet releases still going to SABnzbd or NZBGet, torrents going to Put.io
through putio-sync, and the apps choosing between them the way you set.

This is "keep both", not a switch. Put.io does not take NZBs. It accepts magnet links, torrent
files and URLs. So keep SABnzbd or NZBGet for usenet, and add putio-sync for torrents.

For what is different when Put.io does the downloading (seeding, storage, private
trackers), read
[What changes when Put.io does the downloading](from-qbittorrent.md#what-changes-when-putio-does-the-downloading).

## 1. Add putio-sync

1. If putio-sync isn't running yet, follow [your first sync](first-sync.md).
2. Follow [Connect Radarr and Sonarr](radarr-sonarr.md). It adds putio-sync to each app as a
   **Transmission** download client.
3. Make sure the app can see putio-sync's download folder. Step 2 of that guide covers the
   mounts and the **Remote Path Mapping**.

Leave your SABnzbd or NZBGet entry as it is. Don't disable or delete it.

## 2. Give each client its own category

Each client needs its own category so imports don't mix. For example:

- SABnzbd: category `tv`.
- putio-sync: category `sonarr`, so files land in `<download folder>/sonarr/`.

Do the same in Radarr with names such as `movies` and `radarr`.

## 3. Choose usenet or torrent first

Radarr and Sonarr decide which protocol to use for a release in **Settings › Profiles ›
Delay Profiles**. Each profile sets:

- a **preferred protocol**, Usenet or Torrent, and
- a delay for the other protocol.

Choose one of these:

1. **Usenet first.** Prefer Usenet, with a delay for Torrent. Torrents are the fallback.
2. **Torrent first.** Prefer Torrent, with a delay for Usenet. Usenet is the fallback.

## Check it works

1. In Radarr or Sonarr, run a manual search and grab a torrent release. It shows on
   putio-sync's **Queue** page, and the app imports it from
   `<download folder>/<category>/`.
2. Grab a usenet release. It shows in SABnzbd or NZBGet, as before.

## If something goes wrong

- **Releases from both clients import into the wrong place:** check that each client has its
  own category.
- **The app's test fails for putio-sync:** check the app can reach putio-sync's address and
  port (`15432`). In Docker, use the container name or the host's address, not `localhost`.
- **Downloads finish but never import:** see [path mapping](../radarr-and-sonarr.md#path-mapping).

More: [Radarr and Sonarr](../radarr-and-sonarr.md).
