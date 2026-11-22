# Send torrents from a watch folder

**You'll need:** putio-sync set up and syncing ([your first sync](first-sync.md)), and a
folder that putio-sync can read and write.

**You'll end with:** any `.torrent` or `.magnet` file you drop in that folder added to Put.io
as a transfer, and downloaded by putio-sync when it's done.

This is the same as the "watch folder" or "blackhole" in a local torrent client, so a
browser, a script or another app can keep saving torrents where it always did.

## 1. Choose the folder

- **On the same machine:** any folder, such as `~/Downloads/torrents`.
- **In a container:** the folder mounted at `/watches`, which the [Docker](docker.md),
  [Unraid](unraid.md) and [TrueNAS](truenas.md) guides set up. Use `/watches` in the steps
  below.

The folder must exist before you turn the watcher on.

## 2. Turn on the watcher

1. Open **Settings › Put.io › Torrent watcher**.
2. Under **Watched folders**, enter the folder, one per line.
3. Choose what happens to a file once it's uploaded: **Delete after upload**, or an
   **Archive folder** to move it to. With neither, the file stays, and putio-sync remembers
   it was uploaded.
4. Turn on **Enabled**, and save.
5. Restart putio-sync, which turning the watcher on needs. Changes to the folders later
   apply at the next scan.

## 3. Try it

Save a `.torrent` file, or a `.magnet` file holding one magnet link, in the folder. Within a
minute it shows on the **Transfers** page as a Put.io transfer. When Put.io finishes it, the
next sync downloads it.

## In the config file

```toml
[watch]
enabled = true
paths = ["/watches"]
delete_after_upload = true
# archive_directory = "/watches/done"
```

## If a file isn't picked up

- The **Doctor** page warns when a watched file waits over an hour without becoming a
  transfer.
- A file that isn't a valid torrent or magnet is logged once as invalid, and left alone.
- Check that putio-sync can read the folder: in a container, the user it runs as needs
  access to the host folder.

More: [sync and watch](../sync-and-watch.md).
