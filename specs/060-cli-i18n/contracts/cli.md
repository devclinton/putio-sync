# Contract: CLI language

## Choosing the language

| Order | Source | Form |
|---|---|---|
| 1 | Flag | `--lang <CODE>` or `--lang=<CODE>`, global (before or after the subcommand) |
| 2 | Environment | `PUTIO_SYNC_LANG=<CODE>` |
| 3 | Config | `[ui] language = "<CODE>"` in the config file given by `-c`, `PUTIO_SYNC_CONFIG` or the default path |
| 4 | OS | `LANGUAGE` (each entry), `LC_ALL`, `LC_MESSAGES`, `LANG`; then the OS's preferred languages (macOS, Windows) |
| 5 | Default | `en` |

`<CODE>` is matched case-insensitively, with `_` treated as `-` and any `.encoding`/`@modifier`
dropped: an exact code, else its base language. `C`, `POSIX`, `C.UTF-8` mean "no language" and go
on to the next source without a warning.

### Unknown or unavailable values

A value from sources 1–3 that matches nothing prints, once, on standard error:

```text
putio-sync: language "xx" is not available; using <next pick>. Available: ar, da, de, … zh-TW (see `putio-sync languages`). Use --lang en for English.
```

The message itself is in the language finally chosen. It never changes the exit code and is not
printed with `--quiet`. An unmatched OS locale falls through silently.

A build without the `i18n` feature accepts `--lang`; a non-English pick prints:

```text
putio-sync: this build has English only.
```

## `putio-sync languages`

Lists what this build can show (spec FR-005).

```text
$ putio-sync languages
CODE   LANGUAGE
en     English
ar     العربية
de     Deutsch
…
zh-TW  繁體中文

In use: de (from LANG)
```

`putio-sync languages --json`:

```json
{"current":{"code":"de","source":"os"},"languages":[{"code":"en","name":"English","rtl":false},{"code":"ar","name":"العربية","rtl":true}]}
```

`source` is one of `flag`, `env`, `config`, `os`, `default`. The header row and "In use" line are
translated; codes and names are not.

## What is translated

- Command output written for a person: headings, labels, table headers, status words in tables,
  progress lines, warnings and errors putio-sync writes, prompts.
- Help: `about`, `long_about`, each argument's help, help headings ("Usage:", "Options:",
  "Commands:", "Arguments:"), and the built-in `--help`/`--version` descriptions.
- Argument errors for unknown argument, invalid value, missing required argument, unknown
  subcommand and missing `=`. Other clap errors print clap's English. Exit code stays 2.
- `doctor` findings and suggestions when printed as text.
- The daemon's foreground setup link and its instructions.

## What stays English (byte for byte)

- `--json`, `--csv` and `--output json|csv` output: keys, column headers, enum values, numbers,
  dates.
- Exit codes.
- Log lines and their fields (`tracing`), including the startup debug block on standard error.
- Command, flag and value names; config keys; environment variable names.
- Data: file, folder and transfer names, paths, Put.io and OS error text, plugin and hook output,
  user input.

## Prompts

`[y/N]` prompts show the translated question and the translated `[y/N]`. They accept `y`, `yes`
and the language's own yes words (case-insensitive); anything else is no.

## Formatting

- Columns are padded and truncated by display width (CJK counts 2, combining marks 0).
- Right-to-left text is printed in logical order.
- Numbers, sizes and dates keep today's format in every language.
