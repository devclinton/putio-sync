# Contract: Settings addresses and links

## Addresses

- `#/settings` → General.
- `#/settings/<section>` → that section; unknown → General.
- `#/settings/<section>?field=<address>` → that section, then jump to the field.
  - `<group>.<name>` (a config key): input `#set-<group>-<name>`; `watch.` means the
    `watcher` group. If that input doesn't exist, the group's card.
  - anything else: an element id, e.g. `arr-enabled`, `card-plugins`.

`settingsHref(section, field?)` builds these.

## `sectionOfKey(key)`

Config key → section id: `download.{whitelist_patterns,blacklist_patterns,blocked_extensions,rules}`
→ rules, other `download.*` → downloads, `watch.*` / `cleanup.*` / `storage.*` / `putio.*` →
putio, `hooks*` / `plugins*` / `arr*` → integrations, `network.*` / `tailscale.*` / `api.*` →
network, `logging.*` → advanced, `notifications.*` → general, `precheck.*` → rules; unknown →
general.

## `linkParts(text)`

Returns an array of strings and `{ text, href }`. Each "Settings › <Section title>" and
"Settings › <Section title> › <Card title>" (known titles only, longest match first) becomes a
link to the section, and to `card-<cardId>` when a card is named. Text without such a name is
returned as one string.

## Names in text

Docs, doctor suggestions and CLI messages write "Settings › Section › Card", e.g.
"Settings › Integrations › Plugins", "Settings › Downloads › Verification".
