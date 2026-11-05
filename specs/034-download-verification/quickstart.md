# Quickstart: Download Verification

## Automated

```bash
cargo test -p putio-sync-download verify
cargo test -p putio-sync-core events
cargo test -p putio-sync-api verify
node --test putio-sync-api/ui/verify.test.js
```

Expected:
- a mock file whose first serve is corrupted downloads twice, ends with the good bytes,
  sends `verify.failed`, `verify.redownload`, `verify.passed`, then one `download.finished`;
- with every serve corrupted, the download fails with reason `integrity` after
  `verify_max_redownloads + 1` downloads and leaves nothing at the real name;
- a fresh download's CRC32 comes from the stream (no `verifying` phase is seen);
- a run over good, corrupted and deleted files reports `verified`, `mismatch` (queued with
  `redownload = 1`) and `missing`; pause stops reads; stop ends it with `stopped`.

## By hand (scratch server, mock Put.io)

1. Start a scratch server with the mock UI backend.
2. History › Verification › Verify everything; watch progress; Pause, Resume, Stop.
3. Corrupt a downloaded file (`printf x | dd of=file bs=1 seek=10 conv=notrunc`), run
   Report only: it is listed as a mismatch and not queued; run again without Report only:
   it is queued and, once downloaded, the problem shows as fixed.
4. Doctor: "Download verification" warns while the mismatch is unfixed.
5. `curl -s localhost:<port>/metrics | grep putio_sync_verify_`.
6. Local files: select two files › Verify starts a run of two.
