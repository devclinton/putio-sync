# Feature Specification: Plugins Settings Page

**Feature Branch**: `048-plugins-page`

**Created**: 2026-11-05

**Status**: Draft

**Input**: User description: "plugins settings should have it own page, and then maybe tabs for each enabled plugin and ways to enable disabled plugins(without having to see all the settings for it)"

Added the same day: "the plex plugin should let you copy code easily and also open the plex.tv/link
page after generating and copying code when signing into plex."

## Context

- Lua plugins (features 028 and 031) are folders with `plugin.toml` and `main.lua`. Four
  notification plugins ship with putio-sync, plus Plex, Jellyfin and Emby refresh plugins, and an
  operator can add more to the plugins folder.
- Today every plugin found is a full card inside Settings › Integrations › Plugins, enabled or
  not: its state, description, events, hosts, every setting, time limit, actions, and Send test
  event. With six or more plugins the section is long, and most of it belongs to plugins that
  are off.
- A plugin turns on when its settings are saved with Enabled ticked. That save also approves
  the hosts the plugin will contact with the saved values; until then it shows as "Needs
  approval" and makes no calls.
- Feature 046 split Settings into sections with addresses `#/settings/<section>` and
  `?field=<address>` jumps. Links to `card-plugins` and the text "Settings › Integrations ›
  Plugins" appear in the Logs page, the doctor, docs and the config examples.

## Clarifications

### Session 2026-11-05

Settled by the implementing session from the request and the code (the user was not asked);
each is the recommended option.

- Q: Does Enable save at once, or tick a box that the Save button saves later? → A: At once,
  after the hosts are approved in the confirmation. "Enable" that does nothing until a second
  Save would be easy to miss, and the request is to turn plugins on without their settings.
- Q: Does Disable ask first? → A: Only when that tab has unsaved edits, which would be
  dropped. Turning a plugin off loses nothing saved and Enable undoes it.
- Q: Can a plugin that does not load be enabled from the list? → A: No. It is listed with
  its error and no Enable button until it is fixed and Reload plugins finds it working.
- Q: Is the selected tab part of the address? → A: No. The first tab is selected when the
  page opens; Enable selects the new tab; Reload keeps the selected one. Links still go to
  the Plugins card.
- Q: What does the tab mark mean for a plugin that is enabled but waiting for host approval
  (for example after the config file was edited by hand)? → A: The same mark as broken or
  missing settings, and the tab shows the reason; saving the tab approves the hosts, as today.
- Q: Does the combined button open the link itself or the page that action returns? → A: The
  link the action returns; the Plex plugin returns https://plex.tv/link (it returned an
  app.plex.tv sign-in link before, which the request does not mention).

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Plugins have their own page (Priority: P1) 🎯 MVP

As the operator, I want plugins in their own Settings section, so the Integrations section is
about Radarr, Sonarr and hooks, and I can go straight to plugins.

**Why this priority**: It is the first half of the request and every other story lives on
this page.

**Independent Test**: Open Settings; a "Plugins" entry is in the section list, opens
`#/settings/plugins`, and Integrations no longer holds plugins. Old links still land on the
plugins page.

**Acceptance Scenarios**:

1. **Given** Settings is open, **When** I choose Plugins in the section list, **Then** the
   address is `#/settings/plugins` and the page shows the plugins, Reload plugins, any load
   errors and (with Show advanced) the Plugins folder field.
2. **Given** an old link `#/settings/integrations?field=card-plugins` or a link from the
   `plugins` config key, **When** I open it, **Then** the Plugins section opens and the
   plugins card is highlighted.
3. **Given** text naming "Settings › Integrations › Plugins" (old doctor output or docs),
   **When** it is shown in the UI, **Then** it links to the Plugins section.
4. **Given** the Integrations section, **When** I open it, **Then** it shows Radarr / Sonarr
   and Event hooks only, and its help no longer mentions plugins.

---

### User Story 2 - One tab per enabled plugin (Priority: P1)

As the operator, I want each enabled plugin on its own tab, so I see the settings of one
plugin at a time.

**Why this priority**: It is the core of the request: less to scroll through.

**Independent Test**: With two plugins enabled, the page shows two tabs; each tab shows only
that plugin's settings, actions, test and last run.

**Acceptance Scenarios**:

1. **Given** ntfy and Discord are enabled, **When** I open the Plugins section, **Then** there
   is a tab for each, in name order, and the first is selected.
2. **Given** a tab is selected, **When** I look at it, **Then** it shows what the plugin card
   showed before: version, source and state (with the reason when broken, needs approval or
   missing settings), description, events, the hosts it contacts, every setting by type,
   secrets shown only as set with Replace, the time limit, its action buttons, Send test
   event with the lines it logged, and the last run.
3. **Given** I edit a setting on one tab, **When** I switch to another tab and back, **Then**
   my unsaved input is still there, and Save saves the edits on every tab.
4. **Given** an enabled plugin is broken or needs settings, **When** I look at the tab list,
   **Then** its tab shows a mark so I can see it without opening it.

---

### User Story 3 - Turn a plugin on from a short list (Priority: P1)

As the operator, I want plugins that are off listed briefly with an Enable control, so I can
turn one on without scrolling past every setting of every plugin.

**Why this priority**: It is the second half of the request.

**Independent Test**: With Telegram off, the "Available plugins" list shows Telegram's name,
version and a one-line description and an Enable button, and no settings. Enable asks to
approve its hosts, then Telegram gets a tab.

**Acceptance Scenarios**:

1. **Given** plugins that are found but not enabled, **When** I open the Plugins section,
   **Then** they appear in one list, each with name, version, a one-line description and an
   Enable button, and none of their settings.
2. **Given** I press Enable on a plugin, **When** the confirmation opens, **Then** it names
   the hosts the plugin will contact with its current settings, or says it makes no network
   calls, and says when a host still depends on a server address I have not entered.
3. **Given** the confirmation, **When** I approve, **Then** the plugin is turned on and its
   hosts are approved in one save, it leaves the list and its tab opens selected; **When** I
   cancel, **Then** nothing changes.
4. **Given** a plugin that needs a value it has no default for, **When** I enable it,
   **Then** it gets a tab that says which settings are needed.
5. **Given** unsaved edits on another plugin's tab, **When** I enable a plugin, **Then**
   those edits stay unsaved and in place.

---

### User Story 4 - Turn a plugin off from its tab (Priority: P2)

As the operator, I want to turn a plugin off from its tab, so it goes back to the short list
with its settings kept for next time.

**Why this priority**: The reverse of story 3; without it a plugin can only be turned off by
hand in the config file.

**Independent Test**: Disable Discord from its tab; it moves to the list. Enable it again; its
webhook still reads "set" and its other values are as before.

**Acceptance Scenarios**:

1. **Given** an enabled plugin's tab, **When** I press Disable, **Then** the plugin is turned
   off in one save, its tab goes away and it is in the Available list.
2. **Given** a plugin was turned off, **When** I enable it again, **Then** its saved settings
   and secrets are as they were.
3. **Given** the tab has unsaved edits, **When** I press Disable, **Then** I am asked first,
   because those edits will be dropped.

---

---

### User Story 5 - Copy a sign-in code and open its page (Priority: P2)

As the operator signing in to Plex, I want to copy the code and open plex.tv/link in one
click, so I don't retype the code or look for the page.

**Why this priority**: Added to the request the same day; it makes the one sign-in flow
faster but the page works without it.

**Independent Test**: On the Plex tab press Sign in with Plex; the code shows large with Copy
and "Copy and open plex.tv/link". The second copies the code and opens plex.tv/link in a new
tab; the page keeps waiting and saves the token once the code is entered.

**Acceptance Scenarios**:

1. **Given** a plugin action answers with a code, **When** it is shown, **Then** the code is
   large and has a Copy button.
2. **Given** the answer also has a link, **When** I press "Copy and open <link>", **Then** the
   code is copied and the link opens in a new tab; the plain link is still shown.
3. **Given** the page is served over plain http (not localhost), **When** I press either
   button, **Then** the code is selected and the page says to copy it by hand.
4. **Given** the action keeps waiting (a `next` step), **When** a later step leaves out the
   code or link, **Then** the earlier ones stay until the flow ends.

### Edge Cases

- No plugins found: the page says so, and still offers Reload plugins and the Plugins folder
  field.
- No plugin enabled: no tabs; only the Available list.
- Every plugin enabled: the Available list is not shown.
- A plugin whose manifest does not load (broken, no settings form): it is listed with its
  error and cannot be enabled until it is fixed and reloaded; if it was enabled, its tab
  shows the error.
- Settings saved for a plugin no longer in the folder: kept, and named below the list as
  before.
- Reload plugins: tabs and the list are drawn again from the new folder contents; the
  selected tab stays selected when that plugin is still there, and unsaved edits to plugins
  still there are kept.
- The enable or disable save is refused (for example a saved value is no longer valid): the
  error is shown and nothing moves.
- A long description: the list shows its first line or sentence, cut to one line.
- Phone width: the tabs scroll sideways like the section pills.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: Settings MUST have a Plugins section at `#/settings/plugins`, listed after
  Integrations, with its own help text; the Integrations section MUST no longer hold plugins.
- **FR-002**: Existing addresses MUST keep working: `?field=card-plugins` (with any section in
  the path), the `plugins` config key, and the text "Settings › Integrations › Plugins" MUST
  open or link to the Plugins section.
- **FR-003**: Each enabled plugin MUST have a tab, in name order; the tab MUST hold
  everything the plugin card held (see story 2, scenario 2) except the Enabled tick box.
- **FR-004**: A tab whose plugin is broken, needs approval or misses settings MUST carry a
  visible mark in the tab list.
- **FR-005**: Switching tabs MUST keep unsaved input on every tab; Save MUST save all
  plugins' edits together, as today.
- **FR-006**: Plugins found but not enabled MUST be shown in one "Available plugins" list
  with name, version, a one-line description and an Enable button, and none of their
  settings.
- **FR-007**: Enable MUST first show the hosts the plugin will contact with its current saved
  or default values (or that it contacts none, or that a host depends on a value not yet
  set) and MUST turn the plugin on only after the operator approves.
- **FR-008**: An approved Enable MUST save only that plugin's enabled state (keeping its saved
  settings and approving its hosts), MUST leave other unsaved edits in place, and MUST open
  the plugin's new tab.
- **FR-009**: Each tab MUST have a Disable button that saves only that plugin as off, keeping
  its settings and secrets, and moves it back to the Available list; when the tab has unsaved
  edits it MUST ask first.
- **FR-010**: Reload plugins, the load errors, the note about settings for plugins not in the
  folder, and the Plugins folder field (advanced) MUST stay on the Plugins page.
- **FR-011**: Settings search MUST still find the plugin fields on every tab and the Plugins
  card.
- **FR-012**: Docs and in-app text that say "Settings › Integrations › Plugins" MUST say
  "Settings › Plugins".
- **FR-013**: The UI MUST stay under the 416 KiB bundle limit.
- **FR-014**: A plugin action answer with a code MUST show the code large with a Copy button;
  with a link as well, a button MUST copy the code and open the link in a new tab, and the
  plain link MUST stay.
- **FR-015**: Where the browser can't copy (plain http other than localhost), the code MUST be
  selected and the page MUST say to copy it by hand.
- **FR-016**: While an action flow runs, a code or link from an earlier step MUST stay when a
  later step leaves it out. The bundled Plex sign-in MUST link to https://plex.tv/link.

### Key Entities

- **Plugin (found)**: a plugin in the bundled set or the plugins folder, as the plugin list
  reports it: name, version, description, source, state, error, events, permissions, fields,
  actions, hosts needed, last runs.
- **Plugin settings**: the saved entry for a plugin: enabled, approved hosts, time limit,
  memory, values and secrets. Kept when the plugin is turned off or missing.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: With six plugins found and one enabled, the Plugins page shows the settings of
  one plugin only, and the five others take one line each.
- **SC-002**: Turning a plugin on takes two clicks (Enable, then approve) from the Plugins
  page, with no settings of other plugins in view.
- **SC-003**: Every link that opened the plugins card before this feature opens the Plugins
  page.
- **SC-004**: A plugin turned off and on again has the same saved settings and secrets.
- **SC-005**: The UI grows by no more than 11 KiB and stays under the 416 KiB limit (first set at 5 KiB; stories 1–4 came to 8.6 KiB and story 5 to about 2 KiB: 10,902 bytes in all, leaving about 4 KiB under the cap).

## Assumptions

- Approval stays what it is: saving a plugin as enabled approves the hosts it needs with the
  saved values. The confirmation is where the operator sees and accepts them; no new approval
  step is added to the daemon.
- The plugin list already gives each plugin's description, version, state and permissions,
  so no daemon or API change is needed.
- Enable uses the plugin's saved values or defaults; settings are edited on its tab after.
- The memory limit stays hidden as today (kept as saved).
