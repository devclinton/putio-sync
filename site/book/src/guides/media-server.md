# Play downloads on your TV

**You'll need:** putio-sync set up and syncing ([your first sync](first-sync.md)), and a TV
or player that supports DLNA / UPnP, such as most smart TVs, VLC, Kodi or Infuse.

**You'll end with:** your downloads showing up on the TV as a media server called
`putio-sync`, ready to browse and play.

## 1. Let it reach your network (containers only)

TVs find media servers with multicast, which a container's usual bridge network hides. Give
the container the host's network:

- **Docker Compose:** add `network_mode: host` to the service and remove its `ports:`.
  See [Docker](../docker.md).
- **Unraid:** edit the container and set **Network Type** to **Host**.
- **TrueNAS:** edit the app and turn on **Host Network** under the network settings.

On macOS, Windows and Linux without a container, skip this step.

## 2. Turn it on

1. Open **Settings › Media server** and choose **Turn on**.
2. Under **Shared folders**, choose what to show. The download folder is the usual choice.
   To show only movies, share one category, such as `radarr`.
3. Save. The card shows that it's running, and the address players use.

Anyone on your home network can browse and play what you share. There is no password in
DLNA, so share only what the whole household may see.

## 3. Find it on the TV

Open the TV's media player, or its list of sources, and look for **putio-sync**. In VLC,
it's under **Local Network › Universal Plug'n'Play**. Open it, browse to a file and play it.
Seeking works.

## If the TV can't find it

- Check **Settings › Media server**. "No LAN interface" means the container is still on a
  bridge network; see step 1.
- Make sure the TV and putio-sync are on the same network, not a guest Wi-Fi.
- Some network switches and mesh Wi-Fi systems drop multicast. Restarting the switch often
  brings it back; if it keeps happening, look for an IGMP snooping or multicast setting.
- The card lists players seen in the last hour, which shows whether the TV reached it at all.

More, including streaming straight from Put.io: [Media server](../media-server.md).
