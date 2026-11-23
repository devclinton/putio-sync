# Choose what downloads

**You'll need:** putio-sync set up and syncing ([your first sync](first-sync.md)).

**You'll end with:** only the files you want downloading, folder by folder.

By default putio-sync downloads everything in your Put.io account, except blocked file
types such as `.exe`. You can narrow that down in three ways:

- **Presets** choose file types: `video`, `audio`, `ebooks`, `no-binaries` or `everything`.
- **Folder rules** apply a preset, name patterns or a skip to one Put.io folder and
  everything inside it.
- **Blocked extensions** are file types that never download, anywhere.

## Example: TV shows only, no samples

Say your Put.io account has a `Shows` folder, and you want only the videos and subtitles
from it, without sample clips.

1. Open the **Put.io** page and go into `Shows`. Its card shows the rule that applies now.
2. Choose **Set rule**.
3. Set **Preset** to `video`, and enter `srt` under **Also allow**.
4. Under **Never download**, enter `*sample* OR Sample/*`.
5. Choose **Preview**. It lists what each file below `Shows` would get, and a sample of the
   files that would download. Check that the episodes are there and the samples aren't.
6. Choose **Save**. The rule applies from the next sync.

Files the rule blocks show a `blocked` badge on the Put.io page.

## Skip a folder

To download nothing from a folder, such as one you only stream or share, open it on the
**Put.io** page, choose **Set rule** and turn on **Skip folder**. Its files show a `skipped`
badge.

## See every rule

**Settings › Rules and filters** lists every folder rule, the presets (you can edit them,
or add your own), the blocked extensions, and **Recently blocked**: the files that were left
out and the rule that decided each one. Use it when something you expected didn't download.

## In the config file

The same rules can be written in `config.toml`:

```toml
[[download.rules]]
path = "Shows"
preset = "video"
allow = ["srt"]
exclude = "*sample* OR Sample/*"

[[download.rules]]
path = "Shared"
skip = true
```

[Folder rules](../advanced.md#folder-rules) lists every setting a rule can have, and how
nested rules combine.
