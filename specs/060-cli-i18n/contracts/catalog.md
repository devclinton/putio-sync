# Contract: CLI catalogs

## Files

`putio-sync-cli/i18n/<code>.json`, one per code in the web UI's `LANGUAGES` plus `en.json`.

Each is a flat JSON object, keys sorted, one-space indent and a final newline (as written by
`scripts/i18n-translate.mjs`):

```json
{
 "Downloaded {0} of {1}": "{0} von {1} heruntergeladen",
 "Skip confirmation prompt": "Bestätigung überspringen",
 "y|yes": "j|ja"
}
```

## Keys

- The English text exactly as in the code: a `t!`/`msg!` literal, or a clap `about`/`help`/heading
  string.
- `en.json` maps each key to itself and is generated; never edited by hand.
- Placeholders are `{0}`, `{1}`… numbered in order of the macro's arguments. A translation must use
  the same set (any order). A mismatch fails CI and falls back to English at run time.
- `"y|yes"`: the words a confirmation accepts, `|`-separated; the first is shown in `[y/N]`. English
  `y` and `yes` are always accepted too.
- Product names that are a whole key (`Put.io`, `Radarr`…) are kept as they are, as in 051.

## Checks (`cargo test -p putio-sync-cli --test i18n_catalog`)

| Check | Fails when |
|---|---|
| `en.json` current | It differs from the scan of `t!`/`msg!` literals in `putio-sync-cli/src` and `putio-sync-doctor/src` plus the walk of the clap command tree. Message: "CLI catalog is out of date: run `make i18n`" |
| valid | A catalog isn't a JSON object of strings |
| no extra keys | A catalog has a key `en.json` lacks |
| placeholders | A translation's placeholder set differs from its key's (names the language and key) |
| coverage | Under 90% of keys translated in a catalog |
| file list | Files differ from `LANGUAGES` + `en` |
| no translated logs | `t!(` appears inside a `tracing` macro call |

## Commands

```bash
make i18n             # refresh web UI and CLI en.json (UPDATE_I18N=1 cargo test … for the CLI)
make i18n-translate   # fill missing strings in both; needs atranslate-cli and ATRANSLATE_URL
```

`make i18n-translate` first copies translations for keys the web UI catalog of the same language
already has, then sends only what is still missing.
