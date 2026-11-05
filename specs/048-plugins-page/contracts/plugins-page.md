# Contract: Plugins page addresses and calls

## Addresses

| Address | Opens |
|---------|-------|
| `#/settings/plugins` | Plugins section |
| `#/settings/plugins?field=card-plugins` | Plugins section, card highlighted |
| `#/settings/<any>?field=card-plugins` | Plugins section (the card decides the section) |
| `#/settings?field=plugins` (config key) | Plugins section, card highlighted |
| `?field=<id of a field on a tab>` | Plugins section, that tab selected, field focused |

Text "Settings › Plugins", "Settings › Plugins › Plugins" and the old
"Settings › Integrations › Plugins" link to `#/settings/plugins?field=card-plugins`
(the first to `#/settings/plugins`).

## Calls (all existing)

| When | Call |
|------|------|
| Page opens, Reload | `GET /plugins` |
| Card loads | `GET /settings/plugins` |
| Enable (after approve), Disable | `PUT /settings/plugins` with the saved body, one entry changed; then `GET /settings/plugins`, `GET /plugins` |
| Save | `PUT /settings/plugins` with every tab's edits (unchanged) |
| Send test event, actions | `POST /plugins/test`, `POST /plugins/action` (unchanged) |
