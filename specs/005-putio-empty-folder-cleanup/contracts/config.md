# Config Contract: `[cleanup]`

```toml
[cleanup]
# Remove empty folders from Put.io once a day (daemon only). Off by default.
enabled = false
# Local time (24h HH:MM) of the daily run. The container's TZ decides "local"; UTC if unset.
run_at = "03:00"
# Keep empty folders younger than this, so new transfer folders are not removed.
min_age_hours = 24
```

- Missing section or keys use the defaults above.
- `Config::validate` rejects a bad `run_at` or `min_age_hours > 8760`.
- No environment-variable overrides.
