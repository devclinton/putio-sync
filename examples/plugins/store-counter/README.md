# store-counter

Counts finished downloads per day in the plugin's store, under `per_day`, and reports
today's count from `health`. It is the **Store counter** starter in the plugin workbench.

No settings. It needs the `store` permission only.

Try it in the workbench: run `download.finished` a few times. A dry run works on a copy of
the store, so **Store before** and **Store after** show the change and nothing is saved.
Run `health` to see the count as the doctor would.

See [Writing and debugging plugins](https://devclinton.github.io/putio-sync/docs/plugin-workbench.html).
