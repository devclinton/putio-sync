# Data Model: Usage Profile

## Kind

`movies | tv | music | books | software | other`. Per kind: preset (video, video, audio,
ebooks or audio, everything, none), pre-check minimum (none, none, 5, 1, off, none), video
(subtitles, DLNA note), folder names (R7), default folder name.

## Answers

| Field | Type | Notes |
|-------|------|-------|
| kinds | [Kind] | at least one |
| arr | [radarr\|sonarr] | apps used; tick movies/tv |
| by_hand | bool | added by hand on Put.io |
| watch | path? | watch folder, when ticked |
| language | string | UI language code (051) |

## Override (per recommendation id)

`{ off: bool, value: Value? }`: switched off, or a changed value.

## Recommendation

| Field | Notes |
|-------|-------|
| id | `rule:<folder>`, `precheck_watch`, `delete`, `subtitles`, `watch` |
| kind | the kind for a rule |
| reason | code: `found`, `arr_category`, `new_folder`, `smallest_size`, `software_off`, `arr_imports`, `library`, `video`, `detected` |
| params | e.g. `{folder}`, `{kinds}` |
| value | editable value: preset name, MB (0 = off), bool, language list, path |
| off | switched off by an override |
| settings | `[{key, value}]` it writes |

## Key (typed setting)

`download.rules[<folder>].preset`, `precheck.watch[<path>].min_size_mb`,
`precheck.watch[<path>].enabled`, `download.delete_after_download`, `cleanup.enabled`,
`subtitles.languages`, `subtitles.mode`, `subtitles.excluded_categories`, `watch.enabled`,
`watch.paths`. `Null` value = back to the default.

## ProfileConfig (`[profile]`)

| Field | Notes |
|-------|-------|
| kinds, arr, by_hand, watch, language | the saved Answers |
| folders | kind → folders used (so Settings can run again without Put.io) |
| applied | key string → value the profile last set |

Left out of the saved config while empty, so older configs round-trip unchanged.

## Diff (re-run)

`changes: [{key, before, after, restart}]`, `kept: [{key, current, profile}]`.
