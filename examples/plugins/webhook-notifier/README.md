# webhook-notifier

Posts `download.finished` and `download.failed` events to a webhook as JSON:
`{"type", "name", "time"}`. It is the **Webhook notifier** starter in the plugin workbench.

Settings: **Webhook URL** (secret). Its host must be approved when the plugin is turned on.

Try it in the workbench: open **Run**, pick `download.finished`, press **Run**. A dry run
shows the request it would send, with the URL masked, and sends nothing. **Send for real**
posts it to the approved host.

See [Writing and debugging plugins](https://devclinton.github.io/putio-sync/docs/plugin-workbench.html).
