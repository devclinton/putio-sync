# API contract changes

## POST /putio/cleanup/empty-folders

Response gains two fields (from the flattened report):

```json
{
  "removed_count": 1,
  "removed": [{"id": 6, "path": "/Old", "age_hours": 900}],
  "kept_protected": 1,
  "kept_too_new": 0,
  "kept_by_user": 2,
  "kept": [
    {"id": 5, "path": "/Uploads", "reason": "protected"},
    {"id": 7, "path": "/Movies", "reason": "kept by the rule on Movies"},
    {"id": 9, "path": "/TV/Show/Season 1", "reason": "matches keep pattern \"Season*\""}
  ],
  "failures": [], "scanned": 6, "dry_run": true, "duration_ms": 120
}
```

## GET/PUT /settings/cleanup

Gains `keep_patterns: string[]` (missing in a PUT body means `[]`). A pattern refused by
the checker answers 400 with its message.

## GET/PUT /rules/folder/{id}, GET /rules

`RuleBody` gains `keep: bool | null`; `EffectiveView` gains `keep: {value, from}`.
A PUT applies to the next cleanup run.
