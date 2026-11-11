# tick-poller

Checks a URL every five minutes (`tick_seconds = 300`) and after each sync, and logs when
its HTTP status changes. The last status is kept in the store. It is the **Tick poller**
starter in the plugin workbench.

Settings: **URL to check**. Its host must be approved when the plugin is turned on.

Try it in the workbench: run `on_tick`. A dry run answers the request with the reply you
set (200 and an empty body by default); set the reply to 503 and run again to see the
change logged.

See [Writing and debugging plugins](https://devclinton.github.io/putio-sync/docs/plugin-workbench.html).
