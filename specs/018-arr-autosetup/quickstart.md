# Quickstart: validating Radarr/Sonarr Settings and Auto-Setup

This guide checks the feature end to end. The contracts are in [contracts/](contracts/). The
states are in [data-model.md](data-model.md).

## 1. Automated gate (no network, no Put.io)

```bash
make ci
```

`make ci` must pass. The tests for this feature:

| Test | Covers |
|---|---|
| `cargo test -p putio-sync-core arr` | `save_arr_section` keeps other tables and secrets, keeps the password when empty and the key when empty, never writes env values; legacy `category` loads and is ignored; validation (FR-004, FR-006, FR-015) |
| `cargo test -p putio-sync-arr setup` | against a mockito *arr: create, update in place, unchanged, name clash → `putio-sync (2)`, blackhole only when ticked, host candidates tried in order, a test failure leaves no client, a write failure undoes the change, each FR-026 reason (US1, US5, US8, SC-005, SC-006) |
| `cargo test -p putio-sync-arr discovery` | ping and title probes, tier dedupe, the 10 s deadline, a second run → busy, never requests `initialize.json` (FR-017) |
| `cargo test -p putio-sync-arr categories` | categories read from matching clients, kept on error with their age, `blocklist_server` picks by the cached category (FR-014, SC-004) |
| `cargo test -p putio-sync-arr path_check` | visible, not visible, a remote path mapping applied (FR-025) |
| `cargo test -p putio-sync-api --test arr_settings` | every route in contracts/api.md: auth required, 404/409/400 cases, a config write failure leaves the daemon unchanged |
| `cargo test -p putio-sync-api --test arr_rpc` | the RPC turns on and off and takes a new password without a restart (US3, SC-003) |
| `cargo test -p putio-sync-api --test secrets_sweep` | no key or password in any response or log line from these routes (FR-027, SC-008) |

## 2. Manual check against a real Radarr and Sonarr (local Docker, not the NAS)

This needs Docker on the developer machine.

```bash
docker network create arrtest
```

```bash
docker run -d --name radarr --network arrtest -p 7878:7878 lscr.io/linuxserver/radarr:latest
```

```bash
docker run -d --name sonarr --network arrtest -p 8989:8989 lscr.io/linuxserver/sonarr:latest
```

Run putio-sync from a local image, on the same network, named `putio-sync`, with a database and
`/downloads` mounted in all three containers. Then:

1. **Settings → Radarr / Sonarr.** The card shows the "Download client" form and an empty
   "Applications" list. It has no item counts.
2. **Download client.** Turn it on, click Generate, then Save. `curl -u putio-sync:<pw>` against
   `/transmission/rpc` gets 409 with a session id, without a restart.
3. **Set up automatically.** Discovery lists `radarr:7878` and `sonarr:8989` within 10 s, with
   no duplicates.
4. **Pick Radarr.** Paste its API key and keep "Remember" off. The preview shows "create
   putio-sync, category radarr". Run setup. In Radarr → Settings → Download Clients, the
   `putio-sync` client is there, and its Test passes (SC-001).
5. **Run setup again.** The result is "Already set up". Radarr has exactly one putio-sync client
   (SC-005).
6. **Restore path (R7).** In Radarr, set the client's category to `movies` by hand, then run setup
   with category `radarr`. Check that the update works. Then force a write failure by stopping
   Radarr between preview and setup. Check that the client still has its password: Test in
   Radarr passes. This confirms that a masked `********` value is kept on PUT.
7. **Add Sonarr through the form.** Use host `sonarr`, port 8989 and the key, then Test, then
   Save. The row shows category `sonarr` read from Sonarr, after a Sonarr setup, and "Folder
   visible".
8. **Path check.** Remove the `/downloads` mount from Sonarr and Test it. The result is "Sonarr
   cannot see /downloads", with the fix.
9. **Status page.** It shows the Radarr / Sonarr card with item counts and last contact.
10. **Torrent Blackhole.** Add a Torrent Blackhole in Radarr and run setup with "Disable Torrent
    Blackhole" ticked. The blackhole is disabled, not deleted.

When you're done:

```bash
docker rm -f radarr sonarr putio-sync
```

```bash
docker network rm arrtest
```

## 3. On the NAS (after release, operator-run)

Upgrade, then follow step 4 for each *arr. Expect discovery to find `radarr` and `sonarr`, the
host `putio-sync` to be accepted, and "Folder visible" for `/downloads`. The existing Torrent
Blackhole clients stay enabled unless the box is ticked.
