# Switch from Deluge

**You'll need:** putio-sync set up and syncing ([your first sync](first-sync.md)), Radarr or
Sonarr running with Deluge as their download client, and the app's API key (in the app,
**Settings › General**).

**You'll end with:** Radarr and Sonarr sending new grabs to Put.io through putio-sync, your
old Deluge queue finished, and Deluge removed from the apps.

For what is different when Put.io does the downloading (seeding, storage, private
trackers), read
[What changes when Put.io does the downloading](from-qbittorrent.md#what-changes-when-putio-does-the-downloading).

## 1. Set up putio-sync and connect it

1. If putio-sync isn't running yet, follow [your first sync](first-sync.md).
2. Follow [Connect Radarr and Sonarr](radarr-sonarr.md). It adds putio-sync to each app as a
   new **Transmission** download client. Your Deluge entry stays type **Deluge**.
3. Make sure the app can see putio-sync's download folder. Step 2 of that guide covers the
   mounts and the **Remote Path Mapping**.

To fill in the client by hand instead, see [manual setup](../radarr-and-sonarr.md#manual-setup).

## 2. Match categories

Deluge's Label plugin is what gives Radarr and Sonarr their own labels. Map each Deluge label
to a putio-sync category of the same name. A label `sonarr` becomes the category `sonarr`,
and files land in `<download folder>/sonarr/`. Nested categories such as `movies/4k` work
too.

## 3. Move watch folders

If Deluge watches a folder for .torrent or .magnet files, putio-sync can watch the same
folder.

1. In Deluge, turn off the watch for that folder. Never leave both on, or both grab the
   same file.
2. In putio-sync, open **Settings › Put.io › Torrent watcher** and add the folder under
   **Watched folders**.
3. Turn on **Enabled**, save, and restart putio-sync.

[Send torrents from a watch folder](watch-folders.md) has the details.

## 4. Switch Radarr and Sonarr over

Do this in each app.

1. Open **Settings › Download Clients**. You should see Deluge and the new putio-sync entry.
2. Give putio-sync a higher priority than Deluge. A lower number is a higher priority. Or
   untick **Enable** on the Deluge entry.
3. Don't delete Deluge yet.

New grabs now go to putio-sync. Torrents already running in Deluge keep running there and
finish there. They are not moved to Put.io.

## 5. Let the old queue finish, then remove Deluge

1. Wait until Deluge has nothing left in the app's queue, and the apps have imported what it
   finished.
2. In each app, open **Settings › Download Clients** and delete the Deluge entry.
3. Stop Deluge when you no longer need it for anything else.

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

More: [Radarr and Sonarr](../radarr-and-sonarr.md).
