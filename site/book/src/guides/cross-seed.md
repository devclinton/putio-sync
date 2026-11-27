# Connect cross-seed

**You'll need:** putio-sync set up and syncing ([your first sync](first-sync.md)), and the
download client turned on with a password: **Settings › Integrations › Radarr / Sonarr ›
Download client**. The same page shows the address to use under **Connection**.

**You'll end with:** cross-seed reading putio-sync's items as a qBittorrent client, and a
clear view of what it can't do there.

Read this first: cross-seed exists to seed data you already have on another tracker without
downloading it again. putio-sync doesn't seed; Put.io does, on its own servers, from its own
copy. So the main use of cross-seed **doesn't carry over**. What is left is small.

## What works

- cross-seed can sign in, list the items, and read their files, categories and save paths.
- An injected torrent is added to Put.io as a normal transfer. If Put.io already has the
  data it finishes at once there, and putio-sync then downloads it again as a new item.

## What doesn't

- **No linking and no recheck.** putio-sync never points a torrent at files that are already
  on disk. A recheck request is accepted and ignored.
- **A save path outside the download folder is refused** (409). cross-seed normally injects
  beside the data it matched; that only works when the data is under putio-sync's download
  folder.
- **Tags** such as `cross-seed` are accepted and ignored.
- A torrent injected as paused starts anyway.

If you seed from a local client as well, point cross-seed at that client. Use putio-sync
here only if you want matched torrents added to Put.io.

## 1. Add putio-sync to cross-seed

In cross-seed's `config.js`, give the address with the user name and password in it.
Recent versions of cross-seed 6 take a list of clients:

```js
torrentClients: ["qbittorrent:http://putio-sync:PASSWORD@<putio-sync host>:15432"],
```

Older versions use `qbittorrentUrl` with the same `http://...` address. Check the
cross-seed docs for your version. A password with characters such as `@`, `:` or `/` must
be percent-encoded.

## 2. Check it

Start cross-seed. Its log shows whether it signed in. In putio-sync, the **Status** page
shows the client under **Last contact**.

## If something goes wrong

- **cross-seed can't sign in:** check the user name is `putio-sync`, and the password. After
  5 wrong tries the address is refused for a minute.
- **Every injection fails:** the save path is outside the download folder. See above.

More: [what the qBittorrent API answers](../radarr-and-sonarr.md#the-qbittorrent-api).
