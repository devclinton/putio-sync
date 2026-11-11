# Quickstart: Organise TV Shows on Put.io

## Automated

```bash
cargo test -p putio-sync-core release organise
cargo test -p putio-sync-db organise
cargo test -p putio-sync-download --test organise
cargo test -p putio-sync-api --test organise --test ui_assets
node --test putio-sync-api/tests/ui/organise.test.mjs
```

Expected: naming table in [contracts/naming.md](contracts/naming.md) holds; a preview pass makes
no rename/move call; apply then undo returns the file to its folder and name; "auto" moves in
the pass; a plugin folder answer wins; a download-mode target is refused; the eager UI stays
under 488 KiB.

## By hand (daemon with stream mode on)

1. Settings › Downloads: mode "Stream from Put.io".
2. Put.io page → **Organise** → tick "Organise TV shows", save.
3. Add a TV episode transfer on Put.io; after the next sync the panel lists a planned move.
4. Apply it: Put.io now shows `TV Shows/<Show>/Season NN/<Show> - SNNEEE….ext`.
5. Undo it: the file is back in its release folder with its old name.
