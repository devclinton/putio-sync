# Set up on TrueNAS SCALE

**You'll need:** TrueNAS SCALE 24.10 or later with a pool set up for apps, and a Put.io
account.

**You'll end with:** putio-sync running as an app, downloading your Put.io files to a
dataset.

## 1. Make the datasets

Under **Datasets**, create a dataset for downloads, such as `tank/media/putio`. Optionally
create another for a watch folder. The app keeps its own config on an ixVolume unless you
choose otherwise.

The app runs as user and group `568` (`apps`), so give that user write access to these
datasets. Use **Edit Permissions** with **Use ACL Preset: POSIX_APPS**, or choose **Apply
User** with `apps`.

## 2. Install the app

1. Open **Apps › Discover Apps** and search for **putio-sync** (in the community train).
2. Choose **Install** and fill in:

| Section | Setting | What to choose |
|---------|---------|----------------|
| putio-sync Configuration | **Web UI Token** | Leave empty to use the setup code |
| Network Configuration | **WebUI Port** | `30569` by default. Remember it for step 4 |
| Storage Configuration | **putio-sync Downloads Storage** | **Host Path**, then your downloads dataset |
| Storage Configuration | **putio-sync Watch Folder Storage** | Optional: a host path for `.torrent` and `.magnet` files |
| Storage Configuration | **putio-sync Config Storage** | The default ixVolume is fine |

3. Choose **Install** and wait for the app to show **Running**.

If putio-sync isn't in the catalog yet, use **Discover Apps › Custom App › Install via YAML**
with the [Compose file](https://github.com/devclinton/putio-sync/blob/main/packaging/docker-compose/compose.yaml).
Change its paths to your datasets, and its `user` to `568:568`.

## 3. Find the setup code

Open the app from **Apps › Installed**, then under **Workloads** choose the logs icon for the
putio-sync container. Look for:

```text
  putio-sync needs setting up: no config file
  Open http://localhost:15432/ and enter the setup code:

      ABCD-EFGH-JKLM
```

The `localhost` address in that line is inside the container. Use your TrueNAS address and
the **WebUI Port** instead.

## 4. Run the setup wizard

Choose **Web UI** on the app's page, or open `http://<truenas>:30569/`. Then follow
[your first sync](first-sync.md). When it asks for the download folder, enter `/downloads`.

## Keep it updated

When a new release is out, putio-sync shows it on its **What's new** page, and TrueNAS
shows **Update available** on the app. Choose **Update** on the app's page. Settings and
history are kept on the config storage.

## Next

- Radarr and Sonarr apps need the downloads dataset mounted at the same path. See
  [path mapping](../radarr-and-sonarr.md).
- Metrics are on the **Metrics Port** (`30570`) once turned on in Settings. See
  [Metrics](../metrics.md).
