# Config contract: `[organise]` (feature 023)

```toml
[organise]
# Move TV episodes that stream mode keeps on Put.io into Show/Season NN folders
tv = false
# Put.io folder for shows, from the synced folder
tv_folder = "TV Shows"
# "preview": moves wait for you on the Put.io page; "auto": moved as they arrive
apply = "preview"
# Let plugins' folder and rename rules set the target
use_plugins = true
```

Validation: `tv_folder` must not start with `/` and must not contain `..`. An unknown `apply`
value is a parse error. `doctor` warns when `tv` is on but `download.mode` and every folder rule
download (nothing is streamed, so nothing is organised).

024 adds `movies` and `movies_folder` beside these.
