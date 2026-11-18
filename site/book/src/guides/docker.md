# Set up with Docker and Compose

For any Linux machine or NAS that runs containers, such as Synology Container Manager,
QNAP Container Station, Portainer or Dockge. On Unraid or TrueNAS, their own guides are
simpler: [Unraid](unraid.md), [TrueNAS SCALE](truenas.md).

**You'll need:** Docker with the Compose plugin (`docker compose version` works), and a Put.io
account.

**You'll end with:** putio-sync running in a container that restarts with the machine, and
downloading your Put.io files to a folder on it.

## 1. Make a folder for it

```bash
mkdir -p ~/putio-sync/config ~/putio-sync/downloads ~/putio-sync/watch
cd ~/putio-sync
```

## 2. Write `compose.yaml`

Save this as `~/putio-sync/compose.yaml`:

```yaml
services:
  putio-sync:
    image: ghcr.io/devclinton/putio-sync:latest
    container_name: putio-sync
    restart: unless-stopped
    user: "1000:1000"
    environment:
      TZ: Etc/UTC
      HOME: /config
    ports:
      - "15432:15432"
    volumes:
      - ./config:/config
      - ./downloads:/downloads
      - ./watch:/watches
```

Before you start it:

- Set `user` to the owner of the folders (run `id -u` and `id -g`). Without it, the image
  runs as `99:100`.
- Set `TZ` to your time zone, such as `Europe/London`. Bandwidth schedules and the logs use
  it.
- To put downloads somewhere else, such as a media share, change `./downloads` to that
  path. Keep `./config` on a local disk, not a network share: it holds the state database.

The [full Compose file](https://github.com/devclinton/putio-sync/blob/main/packaging/docker-compose/compose.yaml)
also has the metrics port and a health check.

## 3. Start it

```bash
docker compose up -d
docker compose logs putio-sync
```

The log shows the setup code:

```text
  putio-sync needs setting up: no config file
  Open http://localhost:15432/ and enter the setup code:

      ABCD-EFGH-JKLM
```

`localhost` there means inside the container. From another computer, use this machine's address.

## 4. Run the setup wizard

Open `http://<this-machine>:15432/` and follow [your first sync](first-sync.md). When it
asks for the download folder, enter `/downloads`, which is the folder you mounted in step 2.

## Keep it updated

putio-sync shows new releases on its **What's new** page. To install one:

```bash
cd ~/putio-sync
docker compose pull
docker compose up -d
```

Settings and history are in `./config`, so they're kept.

## Without Compose

The same container with `docker run`:

```bash
docker run -d --name putio-sync --restart unless-stopped \
  --user "$(id -u):$(id -g)" -e HOME=/config -e TZ=Etc/UTC \
  -p 15432:15432 \
  -v ~/putio-sync/config:/config \
  -v ~/putio-sync/downloads:/downloads \
  -v ~/putio-sync/watch:/watches \
  ghcr.io/devclinton/putio-sync:latest
```

To update, `docker pull` the image, then remove and re-run the container with the same
command.

## Next

- The media server needs host networking. See [Docker](../docker.md).
- Radarr and Sonarr in other containers need the downloads folder at the same path. See
  [path mapping](../radarr-and-sonarr.md).
- Smaller, hardened images are listed under [Installation](../installation.md).
