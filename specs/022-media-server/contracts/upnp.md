# Contract: UPnP / DLNA surface

Everything is served on `[dlna] port` (default 8200), on the addresses that `[dlna] bind`
gives. There is no auth. It is read-only: no other method than `GET`, `HEAD` and `POST` on
`/ctl/*` is accepted.

## SSDP (UDP 239.255.255.250:1900)

- **NOTIFY `ssdp:alive`**, for each of these NTs:
  - `upnp:rootdevice`;
  - `uuid:<uuid>`;
  - `urn:schemas-upnp-org:device:MediaServer:1`;
  - `urn:schemas-upnp-org:service:ContentDirectory:1`;
  - `urn:schemas-upnp-org:service:ConnectionManager:1`.

  Each carries `LOCATION: http://<iface addr>:<port>/description.xml`, `CACHE-CONTROL:
  max-age=1800`, `SERVER: <os>/<ver> UPnP/1.0 putio-sync/<version>` and `USN:
  uuid:<uuid>[::<NT>]`. It is sent 3 times at start, then every 900 s.
- **NOTIFY `ssdp:byebye`**: the same NTs, sent on disable, rebind and shutdown.
- **M-SEARCH**: a reply (unicast `HTTP/1.1 200 OK`) goes out when ST is `ssdp:all`, or one of
  the NTs above. The reply is sent after a random delay of at most `min(MX, 5)` s. Other STs
  are ignored.

## HTTP

| Path | Answer |
|------|--------|
| `GET /description.xml` | Device description: `deviceType` MediaServer:1, `friendlyName` = `[dlna] name` (escaped), `manufacturer` putio-sync, `modelName` putio-sync, `UDN` uuid, both services with `SCPDURL`, `controlURL` `/ctl/cds` and `/ctl/cms`, `eventSubURL` `/evt/cds` and `/evt/cms`. |
| `GET /cds.xml`, `GET /cms.xml` | Static SCPDs listing the actions below. |
| `POST /ctl/cds` | ContentDirectory SOAP (see below). |
| `POST /ctl/cms` | ConnectionManager: `GetProtocolInfo` (Source = the protocolInfo strings of every known mime; Sink empty), `GetCurrentConnectionIDs` → `0`, `GetCurrentConnectionInfo` → static. |
| `SUBSCRIBE`/`UNSUBSCRIBE /evt/*` | `200` with `SID: uuid:<random>` and `TIMEOUT: Second-1800`. No events are sent; the `SystemUpdateID` change is picked up by polling. Some TVs refuse a server that rejects SUBSCRIBE. |
| `GET`/`HEAD /media/<ObjectId>` | The media bytes (see Streams). |
| anything else | `404`. |

## ContentDirectory actions

- **`Browse`**:
  - Inputs: `ObjectID`, `BrowseFlag` (`BrowseMetadata` or `BrowseDirectChildren`), `Filter`
    (ignored: everything is always returned), `StartingIndex`, `RequestedCount`,
    `SortCriteria` (ignored: the order is fixed, data-model).
  - Outputs: `Result` (escaped DIDL-Lite), `NumberReturned`, `TotalMatches`, `UpdateID`.
- **`GetSystemUpdateID`**: a counter that goes up on each settings apply.
- **`GetSearchCapabilities`** → empty. **`GetSortCapabilities`** → empty.

Faults (`s:Client`, UPnPError):
- `701` No such object: an unknown, unshared or refused ID, including any traversal.
- `720` Cannot process the request: a Put.io failure.
- `401` Invalid action.

Refusals are logged at WARN with `peer`, `object_id` (truncated to 200 chars) and `reason`.

## Root container (`0`)

There is one container per shared root, in this order: `downloads`, `cat:*` (by name),
`watch:*` (config order), `putio`.

## Streams (`/media/<ObjectId>`)

- The ID is checked as for Browse. Refused → `404`, with no body.
- **Local**: `200` or `206`, with `Accept-Ranges: bytes`, `Content-Type` = item mime,
  `transferMode.dlna.org: Streaming` and `contentFeatures.dlna.org: <4th protocolInfo field>`.
  An unsatisfiable range → `416` with `Content-Range: bytes */<size>`.
- **Put.io**: the same headers. The status, `Content-Range` and `Content-Length` are passed
  through from Put.io, for the MP4 version when it exists and the original otherwise.
  - Put.io error → `502`, logged with the Put.io status.
  - Links and tokens are never in the response.
- `HEAD` returns the headers only, with no Put.io body fetched (for Put.io it uses the cached
  size).

## Metrics (on the existing metrics endpoint)

| Metric | Type | Labels |
|--------|------|--------|
| `putio_sync_dlna_up` | gauge | — (1 when running) |
| `putio_sync_dlna_streams_active` | gauge | `root_kind` = `local` or `putio` |
| `putio_sync_dlna_bytes_sent_total` | counter | `root_kind` |
| `putio_sync_dlna_requests_total` | counter | `kind` = `ssdp`, `description`, `browse`, `stream` or `other`; `result` = `ok`, `refused` or `error` |
