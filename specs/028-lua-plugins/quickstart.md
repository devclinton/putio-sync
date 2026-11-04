# Quickstart: Lua Plugins

1. `make ci` passes (includes the plugins crate tests: sandbox, limits, host API, bundled
   notifiers against mockito).
2. Run a server with a config file; open Settings › Plugins: ntfy, discord, telegram and
   pushover are listed, disabled.
3. Enable ntfy with a topic, save: the form shows "Contacts ntfy.sh"; `approved_hosts`
   appears in the config. Click Send test event; the phone gets "putio-sync test".
4. Create `plugins/loop/plugin.toml` (`events = ["*"]`) and a `main.lua` whose `on_event`
   loops forever; click Reload; enable it with `timeout_secs = 1`; Send test event: the result
   is a timeout after about 1 s and `/metrics` shows
   `putio_sync_plugin_runs_total{plugin="loop",result="timeout"}`.
5. Run the doctor: the Plugins check warns about `loop`'s timeouts.
6. `curl -H "Authorization: Bearer …" /settings/plugins` shows the ntfy token as `********`.
