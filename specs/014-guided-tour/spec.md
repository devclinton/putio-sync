# Feature Specification: Guided tour and in-app help

**Feature Branch**: `feat/014-guided-tour`

**Created**: 2026-11-27

**Status**: Implemented

**Input**: ROADMAP 014: "A short, skippable tour of each page on first visit, re-runnable from
Settings. Help panels and tooltips explain each setting in plain words, with links to the
guides from 016."

## Context

- The web UI has 11 pages in its navigation, behind a hash router. A new user lands on Status
  after the setup wizard with nothing that says what each page is for.
- Settings has 11 sections and 34 cards (33 named in the section list, plus Stream mode). Each section has a one-line description, and fields
  have hints, but no card says in plain words what it is for or when to use it.
- The docs book has guides (016). The UI links to a guide from a few cards only.
- The UI is translated through catalogs keyed by English text (051), works at phone width and
  in both themes, and keeps per-browser choices in `localStorage`.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - A short tour on the first visit to a page (Priority: P1)

The first time a user opens a page in a browser, a small card says what the page is for in two
to four steps, each pointing at one part of the page. The user can step through it, skip it, or
ignore it and use the page.

**Independent Test**: in a fresh browser profile, open Status and step through the tour with
Next; open Queue and press Esc; reload both pages and check that neither tour shows again.

**Acceptance Scenarios**:

1. **Given** a browser that has not shown the Status tour, **When** Status opens, **Then** a
   card shows "Step 1 of 3" with a title and a sentence or two, and the part of the page it
   describes is outlined.
2. **Given** the tour is showing, **When** the user presses Next, **Then** the next step shows;
   on the last step the button reads Done and ends the tour.
3. **Given** the tour is showing, **When** the user presses Esc or Skip, **Then** it closes and
   does not show again for that page in that browser.
4. **Given** the tour is showing, **When** the user clicks a button on the page under it,
   **Then** the button works: the tour has no backdrop and blocks nothing.
5. **Given** the tour is showing, **When** the user goes to another page, **Then** it closes;
   the other page shows its own tour if it has not been seen.
6. **Given** the tour is showing, **When** the user picks "Don't show tours", **Then** it
   closes and no page shows a tour until tours are turned back on from Settings.
7. **Given** the setup wizard or the sign-in page is showing, **Then** no tour shows.

---

### User Story 2 - Help for each group of settings (Priority: P1)

Next to every settings card heading is a **?** button. It opens a short panel under the heading
that says, in plain words, what the group is for, with a link to the guide that covers it.

**Independent Test**: open Settings › Integrations, press the ? next to "Radarr / Sonarr", and
check that the panel names the guide "Radarr and Sonarr" and that the link opens that guide.

**Acceptance Scenarios**:

1. **Given** any settings card, **Then** its heading has a ? button named "Help: <card title>".
2. **Given** the ? is pressed, **Then** a panel opens under the heading with one short paragraph
   and a "Guide: <title>" link, and the button reports that it is expanded.
3. **Given** the panel is open, **When** the ? is pressed again, or Esc is pressed on it,
   **Then** the panel closes.
4. **Given** a card reloads (after Save, Retry or a lazy load), **Then** the ? is still there.

---

### User Story 3 - Show the tours again (Priority: P2)

Settings › General has a "Tours and help" card with a **Show the tours again** button. It
forgets which tours were seen in this browser and starts the Settings tour at once.

**Independent Test**: after seeing the Status tour, press the button, open Status and check
that its tour shows again.

**Acceptance Scenarios**:

1. **Given** every tour was seen or turned off, **When** the user presses Show the tours again,
   **Then** a toast confirms it, the Settings tour starts, and every other page shows its tour
   on its next visit.

---

### Edge Cases

- `localStorage` is blocked or full: a tour shows once per page load and nothing breaks.
- The saved value is not what the UI wrote (hand-edited, another version): it is read as
  "nothing seen".
- A step's part of the page is missing (an empty list, a hidden toolbar): the step still shows,
  without an outline.
- A confirmation dialog or the command palette is open when the tour would start: it does not
  start, and is not marked as seen.
- The user is typing in a field when the tour starts: focus stays in the field.
- Phone width: the card is a sheet along the bottom, above the tab bar.
- Reduced motion: the card appears without animation and nothing scrolls smoothly.
- A page outside the navigation (What's new, Update, sign-in, setup) has no tour.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: Each page in the main navigation MUST have a tour of one to four steps. Each step
  MUST have a title, one or two plain sentences, and a part of the page it points at.
- **FR-002**: A page's tour MUST show the first time the page opens in a browser, and MUST NOT
  show again in that browser once it has been shown, until tours are shown again (FR-008).
- **FR-003**: The tour MUST NOT show on the sign-in, setup or sign-in-provider pages, while the
  daemon is in setup mode, or while a dialog is open.
- **FR-004**: The tour MUST NOT block the page: no backdrop, no focus trap, and controls under
  and around the card stay usable.
- **FR-005**: The tour MUST be closable with Esc and with a Skip button, MUST offer Back and
  Next (Done on the last step), and MUST offer "Don't show tours", which marks every tour seen.
- **FR-006**: The tour MUST be usable with the keyboard and a screen reader: it is announced as
  a labelled, non-modal dialog, takes focus when it opens unless the user is typing, announces
  each step, and returns focus to the page when it closes.
- **FR-007**: The tour MUST fit phone width, MUST work in both themes and in right-to-left
  languages, and MUST NOT animate when the user asks for reduced motion.
- **FR-008**: Settings MUST have a "Show the tours again" control that forgets every seen tour
  in this browser.
- **FR-009**: Every settings card MUST have a ? control that opens a short plain-language help
  panel with a link to a guide from 016. The control MUST state whether its panel is open.
- **FR-010**: Tour and help text MUST be data in one module, so a test can check that every
  navigation page has a tour, every settings card has help, and every guide link names a guide
  that exists.
- **FR-011**: All tour and help text MUST be translatable through the UI's catalogs (051).
- **FR-012**: The tour card and its text MUST load only when a tour is due, so a browser that
  has seen every tour downloads neither.

### Key Entities

- **Tour step**: a page, a title, a text and the selector of the part of the page it points at.
- **Seen tours**: the set of pages whose tour was shown, kept per browser.
- **Card help**: a settings card id, a text and a guide slug.

## Success Criteria *(mandatory)*

- **SC-001**: A new user can say what each page is for after its tour, in under 30 seconds a page.
- **SC-002**: No tour shows twice in a browser unless the user asks for it.
- **SC-003**: Every settings card explains itself in one paragraph and names a guide.
- **SC-004**: Adding a page to the navigation or a card to Settings without a tour or help
  fails a test.

## Assumptions

- The tour's Skip, Back, Next and Done buttons reuse strings the setup wizard already had.
  Their machine translations were wrong in most languages (for example a word for "jump" or
  for a person's back), so this change corrects those four strings in all 26 catalogs. The
  same keys label a few other places (paging, a status), where the new wording also fits.
- "Seen" is kept per browser in `localStorage` (`putio-sync.tours`), like the appearance and
  language choices. It is not synced between devices and not stored on the server.
- A tour counts as seen once it is shown, not only when it is finished, so leaving the page
  mid-tour does not bring it back.
- "Help panels and tooltips" is built as one control: a ? per settings card that opens a panel.
  A panel works with touch, the keyboard and screen readers, which a hover tooltip does not.
  Fields keep their existing hints and "Affects" lines (015); they get no ? of their own.
- Every help entry links to one guide. Where no guide covers the card, it links to the guides
  index.
- What's new, Update, sign-in and setup have no tour: they are one-purpose pages or are shown
  before the user is in the app.
- The tour starts up to 2 seconds after the page opens, once the first step's part of the page
  exists, because pages fill in as their data arrives.
- Out of scope: tours of dialogs and of the setup wizard, per-field help panels, a server-side
  "seen" flag, and videos or images in tours.
