# Feature Specification: Multi-language Web UI

**Feature Branch**: `051-ui-i18n`

**Created**: 2026-11-08

**Status**: Draft

**Input**: User description: "Multi-language web UI. Pull the web UI's English strings into i18n JSON
catalogs (English is the source), translate them with a self-hosted machine-translation service into
every language it supports, add a Language picker in Settings that defaults to the browser language,
switch the UI at runtime without a reload, set the page language and direction (right-to-left for
Arabic and Hebrew), and keep the UI bundle inside the size budget by loading non-English catalogs only
when chosen. Also translate the Snap Store listing if Snapcraft supports localized listings."

## Context

- Every label, button, heading, toast and dialog in the web UI is an English string written where the
  element is built. There is no catalog and no way to show another language.
- The maintainer runs a machine-translation service that translates JSON catalogs. As of this spec
  it has models for 26 base languages besides English (regional variants such as es-MX or fr-CA are
  left out; the base language covers them).
- The UI's total size is capped by a test that `make ci` runs. The cap has been raised per feature
  before (it is 480 KiB now) and is close to full.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - See the UI in my language (Priority: P1)

A user whose browser is set to German opens putio-sync and sees the navigation, pages, buttons,
toasts and dialogs in German, without changing anything.

**Why this priority**: This is the whole value for most non-English users. Most never open Settings.

**Independent Test**: Set the browser language to German, open the UI and check that the navigation
and the Status page are in German.

**Acceptance Scenarios**:

1. **Given** the browser prefers `de-DE` and no language was picked, **When** the UI loads, **Then**
   it shows German text and the page language is `de`.
2. **Given** the browser prefers a language with no catalog (say `ga`), **When** the UI loads,
   **Then** it shows English.
3. **Given** the browser prefers `pt-BR`, **When** the UI loads, **Then** it shows Portuguese (the
   base language serves the regional variant).

---

### User Story 2 - Pick a language in Settings (Priority: P1)

A user opens Settings, picks a language from a list showing each language's own name, and the whole
UI switches at once, without a reload. The choice is remembered in that browser.

**Why this priority**: The browser language is not always what the user wants, and a shared NAS UI
may be used by people with different preferences.

**Independent Test**: Pick French in Settings, check the current page and the navigation switch to
French straight away, reload and check French is kept, then pick "Browser default" and check the UI
follows the browser again.

**Acceptance Scenarios**:

1. **Given** the UI is in English, **When** the user picks Français, **Then** the navigation and the
   open page change to French without a page reload.
2. **Given** the user picked Français, **When** they reload, **Then** the UI is still in French.
3. **Given** the user picks العربية or עברית, **When** the UI switches, **Then** the page reads
   right to left.
4. **Given** a catalog cannot be fetched, **When** the user picks that language, **Then** the UI
   stays readable in English and says the language could not be loaded.

---

### User Story 3 - Keep catalogs current (Priority: P2)

The maintainer changes or adds UI text, runs one command to refresh the English catalog, and runs the
translation service to update the other languages. A check in `make ci` fails when the English
catalog is out of date with the UI's text.

**Why this priority**: Without it the catalogs rot after the next feature.

**Independent Test**: Add a new button label, run `make ci` and see it fail; run the refresh command
and see it pass.

**Acceptance Scenarios**:

1. **Given** a new UI string not in the English catalog, **When** `make ci` runs, **Then** it fails
   and names the refresh command.
2. **Given** a translated catalog lacks a string, **When** that language is shown, **Then** that
   string appears in English and the rest stays translated.

---

### User Story 4 - Translated Snap Store listing (Priority: P3)

Answered, not built: Snapcraft passes only the untranslated (or `xml:lang="en"`) summary and
description to the Snap Store, which keeps one listing per snap. See research.md.

### Edge Cases

- Text that is data, not UI (file names, transfer names, log lines, paths, error text from the
  daemon or Put.io), is never translated, even if it happens to match a UI string.
- Placeholders in messages (counts, names) survive translation in the right place, and a translation
  that drops a placeholder falls back to English for that string.
- Browser storage unavailable: the UI follows the browser language and the picker still switches for
  the session.
- Long translations (German, Finnish) must not break the top bar or the buttons on narrow screens
  beyond what wrapping already allows.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: The UI's English text MUST live in an English catalog that is the source for every
  other language. Text inside the UI code stays English and is the lookup key.
- **FR-002**: The UI MUST ship catalogs for every base language the translation service supports
  (26 at the time of this spec), produced by that service.
- **FR-003**: Without a saved choice, the UI MUST use the first browser-preferred language with a
  catalog, matching the base language when the regional one is missing, and English otherwise.
- **FR-004**: Settings MUST offer a Language picker listing "Browser default", English and every
  shipped language in its own name. The choice MUST be saved in the browser, like the appearance.
- **FR-005**: Changing the language MUST update the navigation and the open page without a reload.
- **FR-006**: The page MUST declare its language and, for Arabic and Hebrew, read right to left.
- **FR-007**: A missing or failing catalog, or a string missing from one, MUST fall back to English.
- **FR-008**: Only UI text MUST be translated. Data shown in the UI MUST NOT be.
- **FR-009**: Non-English catalogs MUST be fetched only when that language is in use, so they do not
  count toward the UI size budget.
- **FR-010**: `make ci` MUST fail when the English catalog is out of date with the UI's text, and
  when a shipped catalog is not valid or has keys the English catalog lacks.
- **FR-011**: The Snap Store listing stays English (Snapcraft keeps one listing per snap).

### Key Entities

- **Catalog**: one language's map from each English UI string to its translation.
- **Language choice**: "Browser default" or a language code, kept per browser.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: With a supported browser language, a first-time user sees the navigation and every
  page heading in that language with no setup.
- **SC-002**: Switching language in Settings takes effect in under one second on a local network.
- **SC-003**: At least 90% of the English catalog's strings are translated in each shipped language.
- **SC-004**: The UI loaded in English is no more than 6 KiB larger than before this feature.

## Assumptions

- Machine translation is acceptable for a first version; native speakers can correct a catalog by
  editing its file.
- Messages built from several values (counts, names) use numbered placeholders; plural forms are not
  modelled. A message whose English wording depends on a count keeps the English plural logic and may
  read less naturally in other languages.
- The language choice is per browser, not a daemon setting, matching the appearance setting.
- Server-generated text (daemon errors, Doctor findings, What's new) stays in English.
