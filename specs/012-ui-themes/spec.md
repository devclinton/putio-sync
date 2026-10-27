# Feature Specification: Web UI Redesign with Themes

**Feature Branch**: `012-ui-themes`

**Created**: 2026-10-26

**Status**: Draft

**Input**: User description: "UI could be a bit more modern. I like minimalism but also like Apple
philosophy of some charm/style there. Probably good to be themable." Direction B ("Tinted") was
picked from two prototypes: a sidebar with icons on desktop, a bottom tab bar on phones, and
background greys that take a hint of the chosen accent colour.

## Context

- The web UI (feature 004, extended by 005–008) is plain HTML, CSS and JavaScript modules
  served by the daemon, with no build step and a 150 KB total size budget. It is about
  124 KB today.
- It already follows the system light/dark setting, through a small set of colour variables.
  There is no manual override and no choice of accent.
- Card headings are small all-caps labels, buttons are flat outlined boxes, and the top bar
  wraps onto three lines on a phone.
- Other features in flight (per-folder rules, a Put.io doctor page, Radarr/Sonarr settings)
  add pages and sections to the same UI. They should get the new look by using the shared
  styles, without per-page work.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - A calmer, more polished UI on every page (Priority: P1)

The operator opens any page (sign-in, Status, Queue, Put.io, Transfers, History, Stats,
Settings) and sees the same visual language: a clear type scale with large page titles,
generous spacing, soft depth and rounded corners, status shown as coloured pills and dots,
and tables that become readable cards on a phone. Navigation is a sidebar with an icon per
page on desktop and a bottom tab bar on a phone.

**Why this priority**: This is the request itself; themes build on it.

**Independent Test**: Open every page against the mock server at desktop and phone widths, in
light and dark, and compare with the before screenshots. Every page uses the new layout, no
page scrolls sideways, and the tab bar does not cover content.

**Acceptance Scenarios**:

1. **Given** a desktop-width window, **When** any signed-in page loads, **Then** the page list
   is a sidebar with an icon and a label per page, the current page is highlighted in the
   accent colour, and Sign out sits at the foot of the sidebar.
2. **Given** a phone-width window, **When** any signed-in page loads, **Then** the page list is
   a bar fixed to the bottom of the screen with an icon and short label per page, every item
   is at least 44 px tall, and the last content on the page can still be scrolled into view
   above the bar.
3. **Given** the Status page, **When** it renders, **Then** card headings are sentence-case
   titles (not all-caps labels), key numbers are large, and healthy / warning / failing states
   are shown with a coloured dot or pill and a text label, never colour alone.
4. **Given** the Queue, Put.io, Transfers or History table at phone width, **When** it renders,
   **Then** each row is a card with the name as its heading and labelled values beneath.
5. **Given** a list with nothing in it (empty queue, empty folder, no transfers, no history),
   **When** it renders, **Then** the page shows a short empty state that says what would
   appear there and, where there is one, the action that fills it.
6. **Given** the Put.io or Transfers page with rows selected, **When** the selection bar shows,
   **Then** it uses the same look and stays usable at phone width.
7. **Given** the operator's system asks for reduced motion, **When** pages, toasts and dialogs
   appear, **Then** nothing slides or fades.

---

### User Story 2 - Pick light, dark or system, and an accent colour (Priority: P1)

The operator switches between light, dark and "match system" from a one-click control in the
navigation, and picks an accent colour from a small set of built-in themes on the Settings
page. The choice applies at once to every page, including the Stats charts, and is still
there after a reload or the next visit from the same browser.

**Why this priority**: Themability was asked for, and a manual dark/light override is the most
common need (for example a dark UI on a light-mode laptop at night).

**Independent Test**: In the mock UI, choose Dark with the Teal accent, reload, and check the
page is still dark and teal; open a second browser profile and check it still follows the
system with the default accent.

**Acceptance Scenarios**:

1. **Given** no saved choice, **When** a page loads, **Then** it follows the system light/dark
   setting with the default (blue) accent, and changes if the system setting changes.
2. **Given** the appearance control in the navigation, **When** the operator activates it,
   **Then** it steps System → Light → Dark → System, the page switches at once, and the
   control's accessible name says the current mode.
3. **Given** the Settings page, **When** it loads, **Then** an Appearance section above the
   daemon settings shows the mode (System / Light / Dark) and one swatch per accent, with the
   current choice marked; it says that the choice is saved in this browser only.
4. **Given** a saved choice, **When** any page loads (including sign-in), **Then** it renders in
   that theme from the first paint, with no flash of the other theme.
5. **Given** the Stats page, **When** the theme or accent changes, **Then** bars, the heatmap
   and the breakdown bars use the new accent and stay readable in both light and dark.
6. **Given** a browser that blocks local storage, **When** the operator changes the theme,
   **Then** it applies for the current page view and the UI does not break; it simply is not
   remembered.

---

### User Story 3 - Accessible in every theme (Priority: P2)

Every combination of mode and accent keeps text readable and controls usable from the
keyboard.

**Why this priority**: A theme that breaks contrast or focus makes the UI worse, not better.

**Independent Test**: An automated check computes the contrast of every text/background and
accent pair in every theme; a keyboard-only pass through each page reaches every control
with a visible focus ring.

**Acceptance Scenarios**:

1. **Given** any mode and accent, **When** body text, muted text, links, status text and
   button labels are measured against the surfaces they sit on, **Then** each meets WCAG AA
   (4.5:1 for text, 3:1 for large text, icons and focus rings).
2. **Given** keyboard-only use, **When** the operator tabs through any page, **Then** every
   control shows a visible focus ring in the accent colour, and the order follows the
   visual order (sidebar or tab bar, then page content).
3. **Given** the appearance control and the accent swatches, **When** used by keyboard or a
   screen reader, **Then** they are real buttons or radio controls with names, and the chosen
   one is announced as selected.

---

### Edge Cases

- A page added later by another feature without its own icon still shows in the sidebar and
  tab bar with a generic icon.
- More than seven pages at phone width: the tab bar items get narrower but stay at least 44 px
  tall; labels may be shortened but every item keeps an accessible name.
- A saved accent name that no longer exists (for example after an upgrade) falls back to the
  default accent.
- Very long file names keep wrapping inside their card or cell; nothing scrolls sideways.
- Printing or a browser without colour-mixing support: the page still shows readable text on
  a plain background.
- The "Can't reach the daemon" banner and toasts stay visible above the phone tab bar.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: Every colour, corner radius, shadow, spacing step and font stack used by the UI
  MUST come from one shared set of named design values, so a theme is a change of values only.
- **FR-002**: The UI MUST offer three appearance modes: match system (default), light and dark.
- **FR-003**: The UI MUST offer at least six built-in accent colours, with blue as the default,
  and a neutral (graphite) option.
- **FR-004**: The background and grey tones MUST take a subtle tint of the chosen accent.
- **FR-005**: The mode and accent MUST be saved in the browser and re-applied before the first
  paint on every page, including sign-in. They MUST NOT be sent to or stored by the daemon.
- **FR-006**: A one-click mode control MUST be available from the navigation on every signed-in
  page; mode and accent MUST both be choosable in an Appearance section on the Settings page.
- **FR-007**: Navigation MUST be a sidebar with icons and labels at desktop widths and a fixed
  bottom tab bar at phone widths (600 px and below).
- **FR-008**: Every page MUST be restyled: sign-in, Status, Queue, Put.io, Transfers, History,
  Stats, Settings, the bulk-selection bar, toasts, the confirm dialog and the offline banner.
- **FR-009**: Tables MUST become labelled cards at phone width.
- **FR-010**: Lists with no rows MUST show an empty state that says what would appear there.
- **FR-011**: Stats charts MUST take their colours from the active theme.
- **FR-012**: Text and controls MUST meet WCAG AA contrast in every mode/accent combination;
  focus MUST be visible on every control; keyboard navigation MUST keep working.
- **FR-013**: Motion MUST be short and subtle, and MUST be turned off when the system asks for
  reduced motion.
- **FR-014**: The whole UI MUST stay within the 150 KB budget, with no external fonts, images or
  build step.
- **FR-015**: Pages added by other features MUST pick up the new look through the shared styles
  without per-page work.

### Key Entities

- **Appearance choice**: mode (system, light or dark) and accent name; kept per browser.
- **Theme**: the set of design values for one mode and accent: surfaces, text, muted text,
  borders, accent and the text that sits on it, status colours, chart colours, shadows.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: All 8 pages render with the new design at 1280 px and 390 px wide, in light and
  dark, with no sideways scrolling.
- **SC-002**: 100% of the theme's text/background pairs pass WCAG AA in all mode/accent
  combinations (checked automatically).
- **SC-003**: Changing mode or accent takes one click (mode) or two clicks (accent) and applies
  to the page within a moment, with no reload.
- **SC-004**: A saved choice shows on the next page load with no visible flash of another theme.
- **SC-005**: The UI's total size stays under 150 KB.

## Assumptions

- Fonts stay as the system font stack (San Francisco on Apple devices); no web fonts, to stay
  inside the budget and keep the daemon self-contained.
- The choice is per browser, not per account: the UI has one shared API token, not users.
- The offline banner, toasts and confirm dialog keep their current behaviour; only their look
  changes.
- The page markup changes only where the new layout needs it (navigation, empty states,
  appearance controls); page logic is untouched.
- Phone width means 600 px and below, the breakpoint the UI already uses.
