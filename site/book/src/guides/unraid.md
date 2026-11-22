# Set up on Unraid

**You'll need:** an Unraid server with Docker enabled, and a Put.io account.

**You'll end with:** putio-sync running as a container, downloading your Put.io files to a
share on the array.

## 1. Add the template

putio-sync isn't in Community Applications yet, so add its template by URL:

1. In the Unraid web UI, open **Docker**. At the bottom of the page, add
   `https://github.com/devclinton/unraid-templates` to **Template repositories** and choose
   **Save**.
2. Choose **Add Container**, and pick **putio-sync** from the **Template** list.

## 2. Check the paths and port

The template fills in sensible defaults. Change them if your shares are laid out
differently:

| Setting | Default | What it is |
|---------|---------|------------|
| **Web UI** | `15432` | The port for the web UI |
| **Downloads** | `/mnt/user/downloads/putio/` | Where finished files go. Inside the container it's `/downloads` |
| **Watch folder** | `/mnt/user/downloads/watch/` | Optional: `.torrent` and `.magnet` files dropped here are sent to Put.io |
| **Config** (advanced) | `/mnt/user/appdata/putio-sync/` | The config file and the state database. Keep it on the cache or another local disk |
| **Web UI token** | empty | Optional. Leave it empty to use the setup code |

Choose **Apply**. Unraid pulls the image and starts the container.

## 3. Find the setup code

On the **Docker** page, click the putio-sync icon and choose **Logs**. Near the top is a
line like:

```text
  putio-sync needs setting up: no config file
  Open http://localhost:15432/ and enter the setup code:

      ABCD-EFGH-JKLM
```

`localhost` there means inside the container. From another computer, use your server's address.

## 4. Run the setup wizard

Choose **WebUI** from the same menu, or open `http://<your-server>:15432/`. Then follow
[your first sync](first-sync.md). When it asks for the download folder, enter
`/downloads`, which is the **Downloads** share you set in step 2.

## Keep it updated

putio-sync tells you on its **What's new** page when a new release is out. To install it, go
to Unraid's **Docker** page, choose **Check for Updates**, then **Apply update** on
putio-sync. Your settings and history are kept in the **Config** folder.

## Next

- Radarr and Sonarr in other containers need to see the same downloads share at the
  same path. See [path mapping](../radarr-and-sonarr.md).
- To use the media server, the container needs **Network Type: Host**. See
  [Docker](../docker.md).
