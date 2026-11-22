# Switch from qBittorrent

**You'll need:** putio-sync set up and syncing ([your first sync](first-sync.md)), Radarr or
Sonarr running with qBittorrent as their download client, and the app's API key (in the app,
**Settings › General**).

**You'll end with:** Radarr and Sonarr sending new grabs to Put.io through putio-sync, your
old qBittorrent queue finished, and qBittorrent removed from the apps.

## What changes when Put.io does the downloading

- **Seeding and ratio.** Put.io downloads and seeds in the cloud. There is no local seeding
  and no share ratio to manage on your home connection. Put.io handles that.
- **Storage.** Files sit in your Put.io storage until putio-sync brings them home, so your
  Put.io plan limits how much can wait there at once.
- **Getting the files.** When Put.io finishes a transfer, putio-sync downloads the files over
  normal HTTPS and tells the app where they are. The app imports them from there.
- **Private trackers.** Put.io's IP address joins the swarm, not yours. Some private trackers
  don't allow that. Check your tracker's rules before you switch it.

## 1. Set up putio-sync and connect it

1. If putio-sync isn't running yet, follow [your first sync](first-sync.md).
2. Follow [Connect Radarr and Sonarr](radarr-sonarr.md). It adds putio-sync to each app as a
   **Transmission** download client, which is how the apps talk to it.
3. Make sure the app can see putio-sync's download folder. Step 2 of that guide covers the
   mounts and the **Remote Path Mapping**.

To fill in the client by hand instead, see [manual setup](../radarr-and-sonarr.md#manual-setup).
The client type is **Transmission** even though you are coming from qBittorrent.

## 2. Match categories

qBittorrent categories map to putio-sync categories of the same name. If Sonarr used the
category `tv-sonarr` in qBittorrent, use `tv-sonarr` in putio-sync. The folder layout then
stays familiar.

qBittorrent keeps a save path for each category. In putio-sync that path becomes
`<download folder>/<category>/`. A category `tv-sonarr` lands in
`<download folder>/tv-sonarr/`. Nested categories such as `movies/4k` work too.

Check that the app can see that folder, as in step 3 of the setup above.

## 3. Move watch folders

If qBittorrent watches a folder for .torrent or .magnet files, putio-sync can watch the same
folder.

1. In qBittorrent, turn off the watch for that folder. Never leave both on, or both grab
   the same file.
2. In putio-sync, open **Settings › Put.io › Torrent watcher** and add the folder under
   **Watched folders**.
3. Turn on **Enabled**, save, and restart putio-sync.

[Send torrents from a watch folder](watch-folders.md) has the details.

## 4. Switch Radarr and Sonarr over

Do this in each app.

1. Open **Settings › Download Clients**. You should see qBittorrent and the new putio-sync
   entry.
2. Give putio-sync a higher priority than qBittorrent. A lower number is a higher priority.
   Or untick **Enable** on the qBittorrent entry.
3. Don't delete qBittorrent yet.

New grabs now go to putio-sync. Torrents already running in qBittorrent keep running there
and finish there. They are not moved to Put.io.

## 5. Let the old queue finish, then remove qBittorrent

1. Wait until qBittorrent has nothing left in the app's queue, and the apps have imported
   what it finished.
2. In each app, open **Settings › Download Clients** and delete the qBittorrent entry.
3. Stop qBittorrent when you no longer need it for anything else.

## Check it works

1. In Radarr or Sonarr, pick a movie or episode and run a manual search. Grab a release.
2. In putio-sync, the item shows on the **Queue** page: first as a Put.io transfer, then as a
   download.
3. When it finishes, the app imports it from `<download folder>/<category>/`.

## If something goes wrong

- **The app's test fails:** check the app can reach putio-sync's address and port (`15432`).
  In Docker, use the container name or the host's address, not `localhost`.
- **Downloads finish but never import:** the app can't see the folder. Run **Set up** again
  on the saved application, or see [path mapping](../radarr-and-sonarr.md#path-mapping).
- **A release fails on Put.io:** with **Remember** ticked at setup, putio-sync blocklists it
  in the app, and the app searches again.
- **Removing an item in the app:** putio-sync cancels the transfer.

More: [Radarr and Sonarr](../radarr-and-sonarr.md).
