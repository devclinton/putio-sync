# When something isn't working

**You'll need:** access to putio-sync's web UI, or to the machine it runs on.

**You'll end with:** the cause found, or a bug report with what's needed to fix it.

## 1. Run the doctor

Open the **Doctor** page and choose **Run now**. It checks the Put.io sign-in, storage,
transfers, the download folder, disk space, the database, the config file and the
connection, and says what's wrong in plain words with a suggestion. Some problems have a fix
button on their line.

The daemon also runs it a couple of minutes after starting, and the Status page shows
**Doctor found a problem** when one does.

From a terminal, run `putio-sync doctor`. In a container:

```bash
docker exec putio-sync putio-sync --config /config/config.toml doctor
```

## 2. Read the logs

Open the **Logs** page. Filter by **Level** or **Search** for a file name.

To see more detail for a while, set **Log level** to `debug`, choose **For how long**, then
**Apply**. It goes back on its own. Choose **Download** to save the lines for a bug report.

## Common causes

| What you see | Usually |
|--------------|---------|
| Nothing downloads, and the doctor says the Put.io sign-in failed | The token was revoked. Paste a new one in **Settings › Put.io** |
| Downloads fail with a permission error | The user putio-sync runs as can't write the download folder. In a container, check its `user` and the host folder's owner |
| A file you expected didn't download | A folder rule or blocked type left it out. See **Recently blocked** in **Settings › Rules and filters** |
| Downloads stop, and the doctor warns about disk space | The disk is nearly full. Free space, or move the download folder |
| Radarr or Sonarr never imports | The app can't see the download folder. See [path mapping](radarr-sonarr.md#2-make-sure-the-app-can-see-the-downloads) |
| Transfers stall on Put.io | The torrent has no seeders, or the account is full. The doctor can re-add stalled ones |
| The web UI doesn't load from another computer | The port isn't published or a firewall blocks it. Try `http://<host>:15432/` on the machine itself |

## Report a bug

If it's still not working, [open an issue](https://github.com/devclinton/putio-sync/issues)
with:

- the version, from the **Status** page;
- how you installed it (Docker, Unraid, the macOS package…);
- what you did, what you expected and what happened;
- the doctor's results, and the log lines from around the problem.

Before you paste anything, remove tokens, passwords and API keys. putio-sync never logs
them, but they can be in your config file or in commands you've run.
