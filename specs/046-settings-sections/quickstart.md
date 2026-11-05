# Quickstart: check Settings sections

1. `cargo test -p putio-sync-api --test ui_js --test ui_assets` (node tests and the 416 KB cap).
2. Run a configured daemon locally (a scratch config with a token, `putio-sync daemon`, or
   `putio-sync server`) and open `http://127.0.0.1:<port>/#/settings`.
3. Desktop width: the side list shows 7 sections; General opens. Pick each section; every card
   loads. Type in Downloads › Workers, switch to Put.io: Downloads shows the unsaved mark; come
   back, the value is there; Save; the mark goes.
4. Open `#/settings/downloads?field=download.verify_read_limit_mb_per_sec`: Downloads opens,
   Show advanced turns on, the field is highlighted.
5. Search "crc", press Enter: Verify downloads is highlighted.
6. Doctor page: a suggestion naming "Settings › Integrations › Plugins" is a link.
7. Phone width (375 px): sections are a row of pills, no sideways page scroll.
