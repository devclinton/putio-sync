# Quickstart: Multi-language Web UI

1. `cargo run -p putio-sync-api --example mock_ui` and sign in with the token it prints.
2. With the browser set to German, the navigation and pages are in German; `<html lang="de">`.
3. Settings › General › Language: pick Français. The page and navigation switch without a reload.
   Reload: still French. Pick العربية: the page reads right to left. Pick Browser default: back to
   the browser's language.
4. Add a label to any page and run `make ci`: `i18n.test.mjs` fails until `make i18n`. Then
   `ATRANSLATE_URL=http://<service>:8400 make i18n-translate` fills the new string in every catalog
   (`LANGS=de,fr` for some).
