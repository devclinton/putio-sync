# Data Model: Listen Addresses and Interfaces

No database changes.

## ListenAddr (core::listen)

| Form | Variant | Example |
|------|---------|---------|
| `IPv4:port` | `Ip(SocketAddr)` | `0.0.0.0:15432`, `192.168.1.20:15432` |
| `[IPv6]:port` | `Ip(SocketAddr)` | `[::]:15432`, `[fd00::5]:15432` |
| `tailnet:port` | `Tailnet { port }` | `tailnet:15432` (case-insensitive keyword) |
| `name:port` | `Interface { name, port }` | `eth0:15432`, `tailscale0:15432` |

Rules: port 1–65535 (0 is accepted only in tests through `Ip`); an interface name is 1–64
characters from `[A-Za-z0-9._-]` and not all digits/dots (so `1.2.3:80` is an error, not an
interface). `Display` gives the same text back.

## BindList (config value)

- TOML: a string (one entry) or an array of strings. Env: comma-separated.
- Serialises one entry as a string, several as an array.
- Rules (`validate_list(field, &[ListenAddr])`): 1–16 entries; no duplicate; per port, an
  IPv4 wildcard excludes other IPv4 `Ip` entries and an IPv6 wildcard other IPv6 `Ip`
  entries.
- Cross rule (`validate_apart`): API and metrics lists share no `Ip` entry, no overlapping
  wildcard/concrete pair, and no identical interface/tailnet entry.

## Config changes

| Key | Before | After |
|-----|--------|-------|
| `[api] bind` | `Option<String>`, `ip:port` | `Option<BindList>`; default `0.0.0.0:15432` |
| `[metrics] bind` | `String`, `ip:port` | `BindList`; default `0.0.0.0:9742` |
| `ApiSettings.bind` | `SocketAddr` | `Vec<ListenAddr>` + `port()` (first entry) |
| `Config::resolve_metrics` | `Resolved<MetricsSettings>` | `Resolved<MetricsListen { settings, bind: Vec<ListenAddr> }>` |
| `Config::api_listen()` | — | the API list after env, with tailnet-only applied |

`tailnet_only = true` → `[Tailnet{p} for each distinct port p of the API list]`.

## Resolution (api::listen)

- `Gives::Addrs(Vec<SocketAddr>)` or `Gives::Waiting(reason)` per entry.
- `Ip` → itself. `Interface` → `interface_addrs` with the port; none → waiting ("no such
  interface eth5" / "eth0 has no address"). `Tailnet` → `tailnet_bind(status, port)` from
  036; error → waiting with its reason.

## Bound

`Arc<RwLock<Vec<SocketAddr>>>` per listener set, in entry order then OS order; updated on
every bind/close. `AppState.listening` is the API's.

## Network settings view

`{ api_bind: [string], metrics_bind: [string] }` (PUT body and GET). See
[contracts/listen-api.md](contracts/listen-api.md) for `/listen` and the check.
