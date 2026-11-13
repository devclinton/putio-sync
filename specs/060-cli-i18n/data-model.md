# Data Model: Multi-language CLI

## Lang

A language code the CLI can show.

| Field | Type | Notes |
|---|---|---|
| code | `&'static str` | `en`, or one of the web UI's `LANGUAGES` (`ar`, `da`, … `pt-PT`, `zh-TW`) |
| name | `&'static str` | The language's own name (`Deutsch`, `日本語`), for `languages` |
| rtl | `bool` | `ar`, `he`; informational only (no reordering) |

The list is one `const` in `core::i18n`; a test checks it equals `LANGUAGES` in
`putio-sync-api/ui/i18n.js` plus `en`.

**Matching** (`Lang::find(input)`): trim, lower-case, `_` → `-`, drop `.encoding` and `@modifier`
(`de_DE.UTF-8@euro` → `de-de`). Exact code first (`pt-pt` → `pt-PT`), then the base before the first
`-` (`pt-br` → `pt`, `zh-hk` → `zh`). `c`, `posix` and empty mean "none". Anything else is unknown.

## Choice

The language in effect for one run.

| Field | Type | Notes |
|---|---|---|
| lang | `Lang` | |
| source | `Source` | `Flag`, `Env`, `Config`, `Os`, `Default` |
| warnings | `Vec<String>` | One per unknown value met on the way (`--lang xx`, a bad config value); printed once on standard error after selection |

Built once by `i18n::select(args, env)` before clap; stored in a `OnceLock`. Taking the args and an
environment reader as parameters keeps it testable.

When the binary was built without the `i18n` feature, any non-English pick becomes `en` with
source `Default` and the warning "this build has English only".

## Catalog

| Field | Type | Notes |
|---|---|---|
| lang | `Lang` | |
| entries | `HashMap<String, String>` | English template → translation |

Loaded for the chosen language only. Lookup misses, and entries whose placeholder set differs from
the key's, return the English template (FR-012). English has no catalog at run time.

## Message

Text that is English when stored or sent, and localized when shown.

An enum:

| Variant | Fields | Notes |
|---|---|---|
| `Template` | `template: &'static str`, `args: Vec<String>` | The English key, with `{0}`, `{1}`; args are already formatted values (data, never translated) |
| `Text` | `String` | Text from elsewhere (an error chain, a plugin); shown as is in every language |

- `Display` → English with args filled in (`Text` as is). `Message` is not `Serialize`; callers
  store `to_string()` where English is wanted.
- `localized()` → the run's language with args filled in (`Text` as is).
- `From<String>` and `From<&'static str>` give `Text`; only `msg!` builds `Template`.

### In the doctor

`Finding.summary` becomes `Message` and `.suggestion` becomes `Option<Message>`.

`CheckResult` (serialized, deserialized, compared) keeps `summary: String` and
`suggestion: Option<String>` in English, filled with `to_string()` in
`putio-sync-doctor/src/lib.rs`, and gains:

| Field | Type | Notes |
|---|---|---|
| local | `Option<LocalText>` | `#[serde(skip)]`, ignored by `PartialEq`; set from the `Finding` when the check runs in this process, `None` after deserializing |

`LocalText { summary: Message, suggestion: Option<Message> }`. The CLI prints `local` when present,
else the English strings. Check titles and area names are `&'static str` literals
(`CheckId::title`, `Area::title`); the CLI looks them up with `i18n::tr` at print time. JSON output
and the API's Doctor responses are unchanged byte for byte.

## UiConfig

`[ui]` section of `Config`.

| Field | Type | Default | Notes |
|---|---|---|---|
| language | `Option<String>` | `None` (automatic) | Stored as written. Config load does not warn; `i18n::select` warns once for the CLI and the doctor's Config check reports it. Readers treat an unknown value as `None` |

## Relationships

```text
argv / env / config [ui] language / OS ──select()──▶ Choice ──▶ Catalog (embedded, one inflated)
                                                                   │
            t!("…", args) ─────────────────────────────────────────┤ lookup
            Message::Template.localized() ─────────────────────────┤
            i18n::tr("Put.io sign-in") (check titles) ─────────────┘
            Message Display ─▶ English String (CheckResult, JSON, API, logs)
```
