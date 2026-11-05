# Contract: manifest additions

```toml
tick_seconds = 5                 # optional, 5..=3600, default 60

[[config]]
key = "libraries"
type = "string_list"
choices = "load_libraries"       # optional; must name an [[actions]] id;
                                 # only for string, url, string_list fields

[[config]]
key = "path_map"
type = "path_map"                # list of {from, to}; cannot be secret
default = []

[[actions]]                      # at most 8
id = "sign_in"                   # [a-z0-9_]{1,40}, unique
label = "Sign in with Plex"      # 1..=40 chars
help = "Opens plex.tv to approve putio-sync."   # optional
hidden = false                   # optional; true = no button (a step reached through
                                 # `next` or a field's `choices` only)
```

Settings (`config.toml`):

```toml
[[plugins]]
name = "plex"
[plugins.config]
path_map = [{ from = "/downloads", to = "/data/media" }]
```
