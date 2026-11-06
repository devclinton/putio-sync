# Research: Plugins Settings Page

## R1 – Does the API need changes?

- **Decision**: No.
- **Rationale**: `GET /plugins` already returns `name`, `version`, `description`, `source`,
  `state`, `error`, `permissions`, `config_fields`, `hosts_needed` and `approved_hosts`.
  `GET /settings/plugins` gives each saved entry with `enabled`. `PUT /settings/plugins` runs
  `check_at`, which sets `approved_hosts` to the hosts an enabled plugin needs, so a save with
  `enabled = true` is the approval.
- **Alternatives**: a `POST /plugins/enable` route. Rejected: it would duplicate the save path
  and its checks.

## R2 – How does Enable save without saving other edits?

- **Decision**: PUT `/settings/plugins` with the last saved body, where only that plugin's
  entry changes (`enabled` flipped; a new entry from defaults when none is saved). Secrets go
  as the mask, so they are kept. Then re-read `/settings/plugins` into the card's saved data
  and `/plugins` for states.
- **Rationale**: the card's Save sends every tab's edits and the folder field; an Enable that
  also saved those would surprise. A targeted body changes one thing.
- **Alternatives**: tick Enabled and press the card's Save (rejected in the clarifications:
  easy to miss and saves unrelated edits).

## R3 – Keeping the "unsaved" mark right

- **Decision**: `form.js` passes `extra(data, { clean, rebase, onSaved })`. The card asks
  `clean()` before a targeted save and calls `rebase()` after it when it was clean, so the new
  saved state is the baseline; when it was dirty the mark stays.
- **Rationale**: the form snapshots its values before the first edit; a targeted save would
  otherwise read as an unsaved change.

## R4 – Unsaved edits across tabs and reloads

- **Decision**: every tab panel is in the DOM, hidden when not selected. Rows (editor state)
  are kept by plugin name across reloads when the plugin's fields are unchanged; the row of a
  plugin just enabled or disabled is rebuilt from its saved entry.
- **Rationale**: hidden panels keep input and let Settings search find their fields. Before
  this feature, Reload dropped all edits.

## R5 – Jumping to a field on a hidden tab, and old addresses

- **Decision**: in `pages/settings.js`, a `card-<id>` address opens the section that holds the
  card, whatever section is in the path; a jump target inside a hidden `[role=tabpanel]`
  clicks the tab named by its `aria-labelledby` first. `linkParts` also maps the old text
  "Settings › Integrations › Plugins" to the new card link.
- **Rationale**: old links (`#/settings/integrations?field=card-plugins`, the 047 Logs page)
  keep working without a list of redirects.

## R6 – Tab look

- **Decision**: a `role=tablist` row of buttons styled like the 046 section pills (always a
  row, scrolling sideways), with a `badge bad` "!" on a tab whose plugin is broken, needs
  approval or misses settings.
- **Rationale**: reuses an existing look; a few CSS lines.

## R7 – One-line description

- **Decision**: the first sentence (up to the first ". " or line break), and CSS cuts it to
  one line with an ellipsis; the full text is the `title`.
