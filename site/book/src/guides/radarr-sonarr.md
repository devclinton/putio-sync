# Connect Radarr and Sonarr

**You'll need:** putio-sync set up and syncing ([your first sync](first-sync.md)), Radarr or
Sonarr running, and the app's API key (in the app, **Settings › General**).

**You'll end with:** Radarr or Sonarr sending releases to Put.io through putio-sync, and
importing them once they're downloaded.

putio-sync acts as a **Transmission** download client for the apps. They send it magnets
and .torrent files, it adds them to Put.io, downloads the results, and tells the app where
the files are.

## 1. Set it up from putio-sync

1. In putio-sync, open **Settings › Integrations › Radarr / Sonarr**.
2. Choose **Set up automatically**. putio-sync looks for Radarr and Sonarr on this machine
   and at their usual container names. If yours isn't found, type its address, such as
   `nas` or `192.168.1.20:7878`.
3. Pick the app, and paste its API key. Tick **Remember** so putio-sync can blocklist
   failed releases in the app later.
4. Check the preview, and choose **Set up**. putio-sync adds itself to the app as a
   download client and tests it.
5. The next step checks that the app can see the download folder. If it can, it shows
   **Folder visible to Radarr** (or Sonarr). If not, see [step 2](#2-make-sure-the-app-can-see-the-downloads).
6. Choose **Done**.

Repeat for the other app. Each app gets its own category, such as `radarr` and `sonarr`,
and each category gets its own folder: `<download folder>/radarr/` and so on.

To set it up by hand in the app instead, see [manual setup](../radarr-and-sonarr.md#manual-setup).

## 2. Make sure the app can see the downloads

Radarr and Sonarr import from putio-sync's download folder, so they need to see it. When
they run in other containers, mount the same host folder at the same path in each one,
such as `/downloads`:

```yaml
services:
  putio-sync:
    volumes:
      - /mnt/user/downloads:/downloads
  sonarr:
    volumes:
      - /mnt/user/downloads:/downloads
```

If the app sees the folder under another path, add a **Remote Path Mapping** in the app.
**Set up** suggests one when it can find the folder. See
[path mapping](../radarr-and-sonarr.md#path-mapping) for the details.

## 3. Check it works

1. In Radarr or Sonarr, pick a movie or episode and run a manual search. Grab a release.
2. In putio-sync, the item shows on the **Queue** page: first as a Put.io transfer, then as
   a download.
3. When it finishes, the app imports it. Its **Activity** page shows the import.

## Prowlarr

Prowlarr manages indexers, not download clients. Add your indexers in Prowlarr and let it
sync them to Radarr and Sonarr as usual. putio-sync needs no Prowlarr setup.

## If something goes wrong

- **The app's test fails:** check the app can reach putio-sync's address and port
  (`15432`). In Docker, use the container name or the host's address, not `localhost`.
- **Downloads finish but never import:** the app can't see the folder. Run **Set up** again
  on the saved application; it checks and fixes the mapping.
- **A release fails on Put.io:** with **Remember** ticked, putio-sync blocklists it in the
  app, and the app searches again.

More on how the client works: [Radarr and Sonarr](../radarr-and-sonarr.md).
