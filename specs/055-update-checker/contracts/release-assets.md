# Release assets contract (055)

What `release-binaries.yml` (053) already publishes stays the same. Its names are fixed
because winget, the docs and the Homebrew formula use them. 055 adds:

| Asset | Per | Content |
|-------|-----|---------|
| `putio-sync-<ver>-<target>.xz` | target | the bare binary (`putio-sync` or `putio-sync.exe`), `xz -9e` |
| `putio-sync-<from>-to-<ver>-<target>.bsdiff.xz` | target × each of the 3 newest earlier releases | bsdiff from `<from>`'s binary to this one, then `xz -9e` |
| `SHA256SUMS` | release | as 053, plus a line for every asset above and one `putio-sync-<ver>-<target>.bin` line per target (the hash of the uncompressed binary; no file by that name is uploaded) |
| `SHA256SUMS.minisig` | release | minisign signature of `SHA256SUMS` |

`<target>`: `linux-x86_64`, `linux-aarch64`, `macos-arm64`, `windows-x64`, `windows-arm64`.
`<ver>` and `<from>`: without the leading `v`.

## Workflow

A new job, `patches`, in `release-binaries.yml`, between the build jobs and `checksums`:

1. Download this run's build artifacts. Pull each bare binary out of its archive and write the
   `.xz`.
2. For the 3 newest earlier published releases (`gh release list --exclude-drafts
   --exclude-pre-releases`), get each target's old binary from its `.xz` asset, or from its
   archive for releases before 055. A target missing from an old release is skipped and logged.
3. Run `cargo run --release -p putio-sync-update --example make-patch -- old new out`, then
   `xz -9e`.
4. Pass everything to `checksums`, which writes `SHA256SUMS` and then signs it:
   `minisign -S -s key -m SHA256SUMS -t "putio-sync <ver>"`. A missing `MINISIGN_SECRET_KEY`
   fails the job.

`make release-patches OLD=<dir> NEW=<dir>` runs steps 3 and 4 (without signing) locally, so the
patch tool can be checked without Actions (Principle II).

## Updater lookup

1. Fetch `SHA256SUMS` and `SHA256SUMS.minisig` and verify them with the built-in public key.
   Stop on failure.
2. If `putio-sync-<running>-to-<ver>-<target>.bsdiff.xz` is listed, download it and check its
   hash, decompress it, and apply it to the running binary's bytes. Otherwise download
   `putio-sync-<ver>-<target>.xz`, check it, and decompress it.
3. Check the result against the `.bin` line. Stop on a mismatch.
