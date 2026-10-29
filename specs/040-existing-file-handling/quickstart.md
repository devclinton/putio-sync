# Quickstart: checking existing-file handling

Nothing here touches the live Put.io account or the NAS.

## 1. Automated

```bash
TMPDIR=<scratchpad> make ci
```

Covers:

- `putio-sync-core`: `ExistingFile` parsing (`rename` alias, unknown value refused),
  `skip_existing` mapping globally and per rule, nearest rule wins, a UI save writes
  `on_existing` and drops `skip_existing` with comments kept.
- `putio-sync-download` unit tests: every cell of the `plan_existing` table
  ([research.md](research.md) R2), with `fixed_path` and `resume_enabled` off; `numbered_name`
  (several dots, no extension, leading dot); `crc32_file` against a known value.
- `putio-sync-download` integration tests against mockito with small payloads, extending
  `tests/interrupted_resume.rs`:
  - half a file at the real name → only the missing range is requested, and the result matches;
  - a resumed file with a bad CRC32 → one full download, then a pass; twice bad → fails;
  - same size and same CRC32 → no request, `completed` row, outcome `skipped`;
  - larger local file in "resume" → old file untouched, new one at `name (1).ext`;
  - "keep both" with `name (1)` taken → `name (2)`; interrupted, then restarted → same name;
  - "overwrite" failing midway → old file byte-identical to before;
  - `fixed_path` with "keep both" → overwrite, no numbered file;
  - target path is a folder → refused, folder untouched;
  - two queue rows for one path → never written at the same time.
- `putio-sync-db`: the four new columns are added to an old database and kept on re-run;
  `fixed_path` stays 1 after a second upsert with 0.
- `putio-sync-api`: settings and rules round-trip `on_existing`; an old body with
  `skip_existing` is mapped; `/history` returns `local_path` and `outcome`.
- `putio-sync-telemetry`: `putio_sync_existing_files_total` has a zero series per outcome.

## 2. UI against the mock

```bash
cargo run -p putio-sync-api --example mock_ui 15532
```

The mock seeds history rows with `saved_as` and `resumed` outcomes and one active download in
phase `checking`. Sign in with the printed token, then:

1. `#/settings`: "When a file already exists" shows Resume. Choose Keep both: the help shows
   `Movie (1).mkv`. Save, reload, and it is still Keep both. The mock's config file has
   `on_existing = "keep_both"` and no `skip_existing`.
2. `#/putio/100` (Movies): the rule card shows the choice as global. Edit → Overwrite → Save: the
   card says "this folder". Clear the rule: it is back to global.
3. `#/history`: the `saved_as` row shows `Movie (1).mkv` under the name with a badge; the
   `resumed` row has its badge; old rows look as before.
4. `#/queue`: the checking download shows "Checking" in place of the speed.
5. Check at 375 px wide and in dark mode: no sideways scroll.

## 3. CLI by hand (temporary folder, mocked or test account only)

```bash
putio-sync download --on-existing keep-both --skip-existing
```

Exits with a usage error naming both flags.
