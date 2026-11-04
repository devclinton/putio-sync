# Contract: Config

```toml
# Optional; default is a "plugins" folder next to this file
plugins_dir = "/config/plugins"

[[plugins]]
name = "ntfy"
enabled = true
approved_hosts = ["ntfy.sh"]   # written by Settings when you enable or save
timeout_secs = 10              # 1-60, per call
memory_mb = 32                 # 4-256

[plugins.config]
topic = "my-nas"
group_minutes = 5

[plugins.secrets]
token = "tk_..."
```

- Unknown plugin names are kept (the folder may come back) and reported by the doctor.
- Config load checks structure only (name, ranges, duplicates, host syntax).
- Values are checked against the manifest on a Settings save and by the doctor.
- `secrets` values are shown as `********` in the settings view and the export.
