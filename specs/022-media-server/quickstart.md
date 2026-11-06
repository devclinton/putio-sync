# Quickstart: Checking the Media Server

Contracts: [upnp.md](contracts/upnp.md), [config.md](contracts/config.md),
[api.md](contracts/api.md). Model: [data-model.md](data-model.md).

## 1. Code checks

```bash
make ci
```

Expected: all pass, including the `putio-sync-dlna` tests:
- discovery;
- browse (sorting, paging, media-only, partials hidden);
- confine (every traversal and symlink case refused, with nothing sent);
- stream (ranges, `416`, file removed mid-stream);
- putio (wiremock: MP4 chosen, original fallback, failure keeps local roots, listing
  refreshed after 60 s).

## 2. Local run

With `[dlna] enabled = true` and `roots = ["downloads"]`, and a small `.mp4`, a `.nfo` and a
`.x.mkv.downloading` in the download folder:

```bash
curl -s localhost:8200/description.xml | grep friendlyName
```

Then run a Browse:

```bash
curl -s -X POST localhost:8200/ctl/cds \
  -H 'Content-Type: text/xml; charset="utf-8"' \
  -H 'SOAPACTION: "urn:schemas-upnp-org:service:ContentDirectory:1#Browse"' \
  --data '<?xml version="1.0"?><s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/"><s:Body><u:Browse xmlns:u="urn:schemas-upnp-org:service:ContentDirectory:1"><ObjectID>downloads/</ObjectID><BrowseFlag>BrowseDirectChildren</BrowseFlag><Filter>*</Filter><StartingIndex>0</StartingIndex><RequestedCount>0</RequestedCount><SortCriteria></SortCriteria></u:Browse></s:Body></s:Envelope>'
```

Expected: only the `.mp4` is listed. Its `res` URL followed by `curl -r 0-99 -o /dev/null -w
'%{http_code}'` prints `206`. `ObjectID` `downloads/..%2F..` gives fault `701`.

## 3. Real players (manual)

- **VLC**: under Local Network › Universal Plug'n'Play, the name shows within 10 s. Play a file
  and seek.
- **Kodi**: Videos › Files › Add videos › Browse › UPnP devices. Browse and play.
- **The TV's media app**: it finds the server and plays an `.mp4` and an `.mkv`.
- Turn the server off in Settings: it disappears from VLC within a minute (byebye).

## 4. NAS (Docker)

- Use `network_mode: host`, as in INSTALLATION.md. Settings › Media server shows "Running" with
  the LAN address, not `172.x`.
- With bridge networking, the status shows the "No LAN interface" warning.

## 5. Put.io root

Set `roots = ["downloads", "putio"]` and `putio_folder` to a folder with one converted video
and one that is not converted:
- Both are listed under "Put.io".
- The converted one plays as MP4 (the log line has `version=mp4`).
- Seeking works.
- Revoke network access to Put.io (or use a bad token): Downloads still browses and plays, and
  the status shows the Put.io error.

## 6. Done when

- Steps 1–2 pass.
- Step 3 passes on VLC and Kodi and one TV.
- Step 5 passes.
- `config.example.toml`, README, INSTALLATION.md and `CHANGELOG.md` are updated.
