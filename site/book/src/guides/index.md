<!--
Guide conventions (feature 016):
- Open with "You'll need" and "You'll end with".
- Number the steps. Quote UI labels in bold, exactly as the UI shows them.
- Link to the reference chapters for details; don't copy them.
- Tokens and passwords are placeholders.
- A guide nobody has walked through on real hardware starts with the
  "Not yet tested on real hardware" note.
-->

# Guides

Step-by-step walkthroughs. Each one starts from where you are and ends with something
working. The rest of these docs explain every setting in detail; the guides link there
when you need it.

## I'm new

Pick the machine putio-sync will run on. Each guide takes you from nothing installed to
your first files downloaded from Put.io.

- [Unraid](unraid.md): the Docker template on an Unraid server.
- [TrueNAS SCALE](truenas.md): the community app.
- [Raspberry Pi](raspberry-pi.md): a Pi 4 or 5 with a 64-bit OS.
- [Docker and Compose](docker.md): any Linux box or NAS that runs containers, such as
  Synology or QNAP. CasaOS, ZimaOS and Umbrel install the same image from their app
  stores.
- [macOS](macos.md): Homebrew or the installer package.
- [Windows](windows.md): the installer.
- [Linux](linux.md): snap, .deb or the standalone binary.

All of them finish with [your first sync](first-sync.md), the setup wizard in the web UI.

## I already use it

Add one piece to a running setup:

- [Connect Radarr and Sonarr](radarr-sonarr.md): putio-sync as their download client.
- [Choose what downloads](folder-rules.md): presets and folder rules.
- [Send torrents from a watch folder](watch-folders.md): drop .torrent and .magnet files in a
  folder.
- [Play downloads on your TV](media-server.md): the built-in DLNA media server.
- [Watch it with Prometheus and Grafana](metrics-grafana.md): metrics and a dashboard.
- [When something isn't working](troubleshooting.md): the doctor, the logs and common causes.

## I'm switching

Move from a local torrent client to Put.io, without losing what's already downloading:

- [Switch from qBittorrent](from-qbittorrent.md): starts with what changes when Put.io does
  the downloading.
- [Switch from Transmission](from-transmission.md): the client type in Radarr and Sonarr
  stays the same.
- [Switch from Deluge](from-deluge.md): Deluge labels become putio-sync categories.
- [Use Put.io next to SABnzbd or NZBGet](from-usenet.md): keep usenet, add torrents.
