# Switch from Transmission

**You'll need:** putio-sync set up and syncing ([your first sync](first-sync.md)), Radarr or
Sonarr running with Transmission as their download client, and the app's API key (in the app,
**Settings › General**).

**You'll end with:** Radarr and Sonarr sending new grabs to Put.io through putio-sync, your
old Transmission queue finished, and Transmission removed from the apps.

putio-sync speaks the same protocol as Transmission, so the client type in the apps stays
**Transmission**. Only the host, port, URL Base and credentials change.

For what is different when Put.io does the downloading (seeding, storage, private
trackers), read
[What changes when Put.io does the downloading](from-qbittorrent.md#what-changes-when-putio-does-the-downloading).

## 1. Set up putio-sync and connect it

1. If putio-sync isn't running yet, follow [your first sync](first-sync.md).
2. Follow [Connect Radarr and Sonarr](radarr-sonarr.md). It adds putio-sync as a second
   **Transmission** client next to your existing one.
3. Make sure the app can see putio-sync's download folder. Step 2 of that guide covers the
   mounts and the **Remote Path Mapping**.

If you add it by hand, the values differ from your old entry like this:

- **Host:** the putio-sync machine.
- **Port:** `15432` instead of `9091`.
- **URL Base:** `/transmission/`.
- **Username:** `putio-sync`. The password comes from **Settings › Integrations › Radarr /
  Sonarr** in putio-sync (**Download client**, with **Generate**).

See [manual setup](../radarr-and-sonarr.md#manual-setup).

Transmission uses port 9091 and putio-sync uses 15432, so both can run on the same machine
with no port clash.

## 2. Match categories

The category you set on Transmission's entry in the app maps to a putio-sync category of
the same name. Reuse it, such as `sonarr`, so the folder layout stays familiar. Files then
land in `<download folder>/sonarr/`. Nested categories such as `movies/4k` work too.

## 3. Move watch folders

If Transmission watches a folder for .torrent files, putio-sync can watch the same folder.

1. In Transmission, turn off the watch for that folder. Never leave both on, or both grab
   the same file.
2. In putio-sync, open **Settings › Put.io › Torrent watcher** and add the folder under
   **Watched folders**.
3. Turn on **Enabled**, save, and restart putio-sync.

[Send torrents from a watch folder](watch-folders.md) has the details.

## 4. Switch Radarr and Sonarr over

Keep both client entries during the switch. Do this in each app.

1. Open **Settings › Download Clients**. You should see two **Transmission** entries.
2. Give putio-sync a higher priority than the old one. A lower number is a higher priority.
   Or untick **Enable** on the old entry.
3. Don't delete the old entry yet.

New grabs now go to putio-sync. Torrents already running in Transmission keep running there
and finish there. They are not moved to Put.io.

## 5. Let the old queue finish, then remove Transmission

1. Wait until Transmission has nothing left in the app's queue, and the apps have imported
   what it finished.
2. In each app, open **Settings › Download Clients** and delete the old Transmission entry.
   Keep the putio-sync one.
3. Stop Transmission when you no longer need it for anything else.

## Check it works

1. In Radarr or Sonarr, pick a movie or episode and run a manual search. Grab a release.
2. In putio-sync, the item shows on the **Queue** page: first as a Put.io transfer, then as a
   download.
3. When it finishes, the app imports it from `<download folder>/<category>/`.

## If something goes wrong

- **The app's test fails:** check the host and that the port is `15432`, not `9091`. In
  Docker, use the container name or the host's address, not `localhost`.
- **Downloads finish but never import:** the app can't see the folder. Run **Set up** again
  on the saved application, or see [path mapping](../radarr-and-sonarr.md#path-mapping).
- **A release fails on Put.io:** with **Remember** ticked at setup, putio-sync blocklists it
  in the app, and the app searches again.

More: [Radarr and Sonarr](../radarr-and-sonarr.md).
