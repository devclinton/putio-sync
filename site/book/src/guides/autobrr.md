# Connect autobrr

**You'll need:** putio-sync set up and syncing ([your first sync](first-sync.md)), and the
download client turned on with a password: **Settings › Integrations › Radarr / Sonarr ›
Download client**. The same page shows the address to use under **Connection**.

**You'll end with:** autobrr sending the releases its filters match to Put.io through
putio-sync, which downloads them when Put.io is done.

putio-sync answers as a **qBittorrent** client for autobrr.

## 1. Add the download client

1. In autobrr, open **Settings › Download Clients › Add new**.
2. Choose the type **qBittorrent**.
3. Host: `http://<putio-sync host>:15432`. In Docker, use the container name or the host's
   address, not `localhost`.
4. Username: `putio-sync`. Password: the download client password from putio-sync.
5. Leave **Basic auth** off, and choose **Test**.

## 2. Use it in a filter

1. Open the filter, then **Actions › Add new**.
2. Choose the action type **qBittorrent** and the client from step 1.
3. Set a **Category** if you want the downloads in their own folder: the category `autobrr`
   downloads to `<download folder>/autobrr/`.
4. Leave **Save path** empty, or give a folder inside putio-sync's download folder.

## What works differently

- The item is a Put.io transfer, so there is no announce to wait for. putio-sync reports
  its tracker as working, and autobrr's reannounce settings have nothing to do.
- Tags, speed limits, ratio and seed time limits, **Skip hash check** and **Add paused** are
  accepted and ignored. Put.io seeds by its own rules.
- **Max active downloads** counts the items Put.io or putio-sync is downloading.
- A release the [release check](../release-check.md) refuses is an add that fails, which
  autobrr shows as a failed action.

## If something goes wrong

- **The test fails with 403 or "Fails.":** the user name or password is wrong. After 5 wrong
  tries the address is refused for a minute.
- **The test fails with 404:** the download client is off in putio-sync. Turn it on and save.
- **The action fails with 409:** the save path is outside putio-sync's download folder, or
  Put.io refused the transfer. putio-sync's log has the reason.

More: [what the qBittorrent API answers](../radarr-and-sonarr.md#the-qbittorrent-api).
