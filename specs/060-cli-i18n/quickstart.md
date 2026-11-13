# Quickstart: Multi-language CLI

Checks that prove the feature end to end. Contracts: [cli](contracts/cli.md),
[config](contracts/config.md), [catalog](contracts/catalog.md).

## Prerequisites

```bash
cargo build -p putio-sync-cli
```

Use a scratch config so nothing touches a real install:

```bash
export PUTIO_SYNC_CONFIG=$PWD/target/i18n-check.toml
```

## 1. OS locale (US1)

```bash
LANG=de_DE.UTF-8 target/debug/putio-sync languages
LANG=pt_BR.UTF-8 target/debug/putio-sync --help
LANG=C target/debug/putio-sync --help
```

Expected: German header and "In use: de (from LANG)"; Portuguese help; English help.

## 2. Order of sources (US2)

```bash
printf '[ui]\nlanguage = "it"\n' > "$PUTIO_SYNC_CONFIG"
LANG=de_DE.UTF-8 target/debug/putio-sync languages              # it (config)
LANG=de_DE.UTF-8 PUTIO_SYNC_LANG=es target/debug/putio-sync languages   # es (env)
PUTIO_SYNC_LANG=es target/debug/putio-sync --lang fr languages  # fr (flag)
target/debug/putio-sync --lang xx languages; echo "exit $?"     # warning, it, exit 0
```

## 3. Machine output is unchanged (US3)

```bash
for l in en de ja ar; do target/debug/putio-sync --lang $l languages --json | shasum; done
cargo test -p putio-sync-cli --test i18n_cli
```

Expected: one hash. The test also compares `status --json`, `files list --csv`, `queue list
--json` and `doctor --json` against mocks, and exit codes.

## 4. Help and argument errors (US4)

```bash
target/debug/putio-sync --lang de download --help
target/debug/putio-sync --lang de download --nope; echo "exit $?"
```

Expected: German descriptions with `--dry-run` and other names unchanged; a German "unknown
argument" message suggesting the closest flag; exit 2.

## 5. Tables with wide characters

```bash
cargo test -p putio-sync-cli output::
```

Expected: the test with a CJK file name over 40 columns passes (it panicked before).

## 6. Catalogs current (US5)

Add `t!("Quickstart check")` to any command, then:

```bash
cargo test -p putio-sync-cli --test i18n_catalog   # fails: run `make i18n`
make i18n && cargo test -p putio-sync-cli --test i18n_catalog   # passes
```

Revert the line and run `make i18n` again.

## 7. English-only and lite builds (US6)

```bash
cargo build -p putio-sync-cli --release --no-default-features --features hooks --target-dir target/lite-en
cargo build -p putio-sync-cli --release --no-default-features --features hooks,i18n --target-dir target/lite
target/lite-en/release/putio-sync --lang de languages
ls -l target/lite-en/release/putio-sync target/lite/release/putio-sync
```

Expected: "this build has English only" and only `en` listed; size difference ≤ 400 KiB, and the
English-only build within 20 KiB of the previous release's lite binary.

## 8. Start-up time (SC-005)

```bash
hyperfine -w 5 'target/release/putio-sync --version' 'LANG=ja_JP.UTF-8 target/release/putio-sync --version'
hyperfine -w 5 -i 'target/release/putio-sync health' 'LANG=ja_JP.UTF-8 target/release/putio-sync health'
```

Expected: each gap is under 10 ms (`-i` because `health` exits non-zero without a daemon). Compare
against the previous release's binary for the "before" figure.

## 9. Web UI default (FR-006)

Run the server with `[ui] language = "fr"`, open the UI in a fresh browser profile set to English:
it shows French. Pick Deutsch in Settings › Language: German, kept on reload. Pick Automatic: French
again.

## Done

`make ci` passes.
