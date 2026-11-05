# Quickstart: Plugins Settings Page

1. `cargo test -p putio-sync-api` (includes the node UI tests and the bundle cap).
2. Run a local daemon with a config (see README › Development) and open the UI.
3. Settings: a **Plugins** entry follows Integrations; Integrations shows only Radarr / Sonarr
   and Event hooks.
4. With no plugin enabled: no tabs; **Available plugins** lists ntfy, Discord, Telegram,
   Pushover, Plex, Jellyfin, Emby with version, one line and **Enable**.
5. Enable ntfy: the dialog says it will contact ntfy.sh; Cancel changes nothing; Enable turns
   it on and opens its tab. Plex: the dialog names the server address field still to fill.
6. On the ntfy tab: settings, time limit, Send test event, last run, Disable. Type in a field,
   switch tabs and back: the text is still there; the section shows "unsaved".
7. Disable ntfy with no edits: it goes back to the list. Enable it again: its topic is kept.
8. Open `#/settings/integrations?field=card-plugins`: the Plugins section opens. The Logs
   page's plugin link and the doctor's plugin hint go there too.
9. Search "topic": the hit opens the ntfy tab and focuses the field.
10. Phone width (375 px): tabs scroll sideways; the list rows fit.
11. Enable plex, press **Sign in with Plex**: the code shows large with **Copy** and **Copy and
    open plex.tv/link**, plus the plain link. The second copies the code and opens plex.tv/link
    in a new tab; the link stays while the page waits. Over plain http to another host, the
    page selects the code and says to copy it by hand.
