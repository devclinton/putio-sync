# Feature Specification: Multi-language CLI

**Feature Branch**: `060-cli-i18n`

**Created**: 2026-11-13

**Status**: Draft

**Input**: User description: "Translate the putio-sync command-line interface (help and about text,
command output such as status, doctor, list, queue, transfers, setup and login prompts, table
headers, progress and error messages) into the same languages as the 051 web UI, through the same
translation service and catalogs keyed by English text. Language is chosen by a `--lang` flag, then
a `PUTIO_SYNC_LANG` environment variable, then a UI language setting in the config (the one 056
saves), then the OS locale, falling back to English. Machine-readable output stays English and
stable so scripts do not break. A check fails `make ci` when the CLI's English catalog is out of
date. Catalogs are built into the binary; the lite builds must stay small, possibly with an
English-only build. Right-to-left languages print in logical order. Daemon messages shown in the web
UI are out of scope except where the CLI prints them itself."

## Context

- 051 translated the web UI into every base language the maintainer's translation service supports
  (26 besides English, plus `pt-PT` and `zh-TW`), with catalogs keyed by the English text. It left
  messages from the daemon in English.
- Every message the CLI prints is an English string written where it is printed: around 300 print
  calls across 20 commands, plus the help and about text of each command and flag.
- The same binary is a CLI and a daemon. The lite builds (CLI only, with or without plugins) are
  published for people who want a small download.
- Scripts, cron jobs, Radarr/Sonarr custom scripts and the bundled plugins read the CLI's `--json`
  and `--csv` output and its exit codes. Logs are read by the web UI's log viewer (047), grep and log
  shippers.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - See the CLI in my language (Priority: P1)

A user whose system is set to German runs `putio-sync status`, `putio-sync files list` or
`putio-sync doctor` and reads the headings, table headers, labels, progress lines and error messages
in German, without changing anything.

**Why this priority**: This is the whole value for most non-English users, and most never pass a
flag or edit the config.

**Independent Test**: Run `LANG=de_DE.UTF-8 putio-sync status` and check the labels are German;
run it with `LANG=C` and check it is English.

**Acceptance Scenarios**:

1. **Given** the OS locale is `de_DE.UTF-8` and nothing else sets a language, **When** the user runs
   `putio-sync status`, **Then** the labels and headings are in German.
2. **Given** the OS locale is `pt_BR.UTF-8`, **When** a command runs, **Then** it prints Portuguese
   (the base language serves the regional one); with `pt_PT` it prints the `pt-PT` catalog.
3. **Given** the OS locale is `C`, `POSIX` or a language with no catalog, **When** a command runs,
   **Then** it prints English.
4. **Given** a German locale, **When** a command fails (bad token, Put.io unreachable, file not
   found), **Then** the message putio-sync adds is in German and any text quoted from Put.io or the
   OS stays as it came.

---

### User Story 2 - Pick the language myself (Priority: P1)

A user picks the language for one command with `--lang fr`, for a shell or a container with
`PUTIO_SYNC_LANG=fr`, or for the install with the UI language setting in the config, which the web
UI and the setup wizard (056) also use.

**Why this priority**: The OS locale is often wrong for the user: containers and NAS boxes default
to `C` or English, and a shared machine may have several users.

**Independent Test**: With a German OS locale, run `putio-sync --lang fr status` and see French; set
`PUTIO_SYNC_LANG=es` and see Spanish; set the config language to Italian with no flag or variable and
see Italian.

**Acceptance Scenarios**:

1. **Given** any locale, **When** the user passes `--lang fr`, **Then** the command prints French.
2. **Given** `PUTIO_SYNC_LANG=es` and no `--lang`, **When** a command runs, **Then** it prints
   Spanish, whatever the config and OS say.
3. **Given** the config's UI language is `it`, no flag and no variable, **When** a command runs,
   **Then** it prints Italian, whatever the OS says.
4. **Given** `--lang xx` (no such catalog), **When** the command runs, **Then** it prints English
   and one warning on standard error naming the languages it has, and still exits as it would
   have in English.
5. **Given** the user runs `putio-sync languages`, **When** it runs, **Then** it prints each
   available code with the language's own name, and which language is in use and why.

---

### User Story 3 - Scripts keep working in any language (Priority: P1)

A script, cron job or *arr custom script runs `putio-sync status --json`, `putio-sync files list
--csv` or checks `putio-sync health`'s exit code on a machine with a French locale, and gets the same
output and exit codes as on an English one.

**Why this priority**: Translating would otherwise break every integration that reads the output,
silently, on exactly the machines of non-English users.

**Independent Test**: Run each command that has `--json` or `--csv` under `LANG=en_US.UTF-8` and
`LANG=ja_JP.UTF-8` and compare the outputs and exit codes byte for byte (with the same data).

**Acceptance Scenarios**:

1. **Given** a non-English language is in use, **When** a command prints `--json` or `--csv`,
   **Then** keys, column headers, enum values (states, kinds), number and date formats are exactly
   those printed in English.
2. **Given** a non-English language, **When** a command exits, **Then** its exit code is the same as
   in English.
3. **Given** a non-English language, **When** the CLI or daemon writes log lines, **Then** the
   messages and structured fields are English, so the log viewer, grep patterns and log shippers
   keep working.

---

### User Story 4 - Translated help (Priority: P2)

A user runs `putio-sync --help` or `putio-sync download --help` and reads the command descriptions,
flag descriptions and section headings in their language. Command names, flag names and value names
stay English.

**Why this priority**: Help is where a new CLI user starts, but most users of the daemon reach the
CLI only through the web UI or a guide.

**Independent Test**: Run `putio-sync --lang de download --help` and check the descriptions are
German and `--dry-run`, `--output` and the like are unchanged.

**Acceptance Scenarios**:

1. **Given** a German language, **When** the user asks for help on any command, **Then** its
   description and each flag's description are in German.
2. **Given** any language, **When** help is shown, **Then** command names, flag names, value names
   and examples are unchanged.
3. **Given** a German language, **When** the user mistypes a flag, **Then** the "unknown argument"
   message putio-sync controls is in German.

---

### User Story 5 - Keep CLI catalogs current (Priority: P2)

The maintainer adds or changes a CLI message, runs one command to refresh the CLI's English catalog
and the translation command to fill the other languages. `make ci` fails when the English catalog is
out of date with the CLI's text.

**Why this priority**: Without it the CLI catalogs rot after the next feature, as 051's would have.

**Independent Test**: Add a new message to a command, run `make ci` and see it fail naming the
refresh command; run the refresh and see it pass.

**Acceptance Scenarios**:

1. **Given** a new CLI message not in the English catalog, **When** `make ci` runs, **Then** it fails
   and names the refresh command.
2. **Given** a translated catalog lacks a message, **When** that language is in use, **Then** that
   message prints in English and the rest stays translated.
3. **Given** a translated message drops or adds a placeholder, **When** `make ci` runs, **Then** it
   fails naming the language and message; at run time that message prints in English.

---

### User Story 6 - Small and English-only builds (Priority: P3)

Someone who wants the smallest binary builds or downloads one with only English, and the lite
builds with the catalogs stay close to their current size.

**Why this priority**: The lite builds exist to be small, but most users of them still benefit from
their own language.

**Independent Test**: Compare the size of each published binary before and after; build the
English-only variant and check `--lang de` prints English with the "not available" warning.

**Acceptance Scenarios**:

1. **Given** the published full and lite binaries, **When** their sizes are compared with the
   previous release, **Then** each grows by no more than the limit in SC-004.
2. **Given** a build with translations left out, **When** any language is asked for, **Then** it
   prints English, warns once that this build has English only, and behaves otherwise the same.

### Edge Cases

- Data is never translated, even when it matches a message: file and folder names, transfer names,
  paths, Put.io error text, OS error text, plugin and hook output, values the user typed.
- Output piped to another program but without `--json` or `--csv` (for example `putio-sync files
  list | grep`) is translated like terminal output; only the machine formats are fixed. The help for
  `--output` and the README say so.
- Terminals that cannot show the language's characters (no UTF-8 locale) still get the chosen
  language; putio-sync does not guess. `--lang en` is the way out and the warning for an unknown
  language mentions it.
- Arabic and Hebrew are printed in logical order and left to the terminal to lay out; table columns
  are padded by display width so wide (CJK) and combining characters keep columns lined up.
- Numbers, sizes and dates in human output keep today's format (for example `1.2 GiB`, ISO dates);
  local number and date formats are not part of this feature.
- The daemon run in the foreground translates the setup link's instructions and the lines saying
  where it listens. Its startup diagnostics block on standard error and its log lines stay English.
- Doctor check titles, findings and suggestions printed by `putio-sync doctor` are translated; the
  same findings in `doctor --json`, on the web UI's Doctor page and in notifications stay English
  for now.
- A wrong value in `PUTIO_SYNC_LANG` or the config is reported the same way as a wrong `--lang`.
- Messages with counts keep the English plural logic, as in 051.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: The CLI's English text MUST live in a CLI English catalog that is the source for
  every other language. Text in the code stays English and is the lookup key.
- **FR-002**: The CLI MUST offer every language the web UI offers (051), translated by the same
  service.
- **FR-003**: The language MUST be chosen in this order: the `--lang` flag, then the
  `PUTIO_SYNC_LANG` environment variable, then the UI language setting in the config, then the OS
  locale (`LC_ALL`, `LC_MESSAGES`, `LANG` on Linux and macOS; the user's display language on
  Windows), then English. A regional code MUST fall back to its base language when there is no
  regional catalog.
- **FR-004**: An unknown or unavailable language from any source MUST fall back to the next source
  and print a single warning on standard error naming the available languages; it MUST NOT change
  the exit code.
- **FR-005**: `putio-sync languages` MUST list the languages the build has, each with its code and
  its own name, and say which one is in use and where it came from.
- **FR-006**: The config MUST have one UI language setting, shared with the web UI's default and the
  setup wizard (056). A language picked in a browser still wins in that browser, as in 051.
- **FR-007**: Human-readable output MUST be translated: command output, labels, table headers,
  prompts (login, setup, confirmations), progress lines, warnings and error messages putio-sync
  writes, the help and about text of commands and flags, and the argument errors putio-sync controls.
- **FR-008**: `--json` and `--csv` output, exit codes, log messages and structured log fields MUST
  be identical whatever the language.
- **FR-009**: Confirmation prompts MUST accept the English answers (`y`, `yes`, `n`, `no`) in every
  language, as well as the translated ones shown in the prompt.
- **FR-010**: Command names, flag names, value names, config keys and environment variable names
  MUST NOT be translated.
- **FR-011**: Data MUST NOT be translated (see Edge Cases).
- **FR-012**: A message missing from a catalog, or whose placeholders do not match the English one,
  MUST print in English.
- **FR-013**: Tables in human output MUST align columns by display width, so CJK and combining
  characters do not break them.
- **FR-014**: Catalogs MUST be built into the binary; the CLI MUST NOT need files or network access
  to show a language.
- **FR-015**: It MUST be possible to build a binary with English only, which behaves as FR-004 says
  when another language is asked for.
- **FR-016**: `make ci` MUST fail when the CLI English catalog is out of date with the CLI's text, and
  when a shipped CLI catalog is invalid, has keys the English one lacks, or changes a message's
  placeholders.
- **FR-017**: The `make` commands that refresh and translate the web UI's catalogs MUST also do the
  CLI's (or have CLI siblings next to them), using the same translation service settings.
- **FR-018**: Strings that the web UI and the CLI both show MUST NOT need translating twice.

### Key Entities

- **CLI catalog**: one language's map from each English CLI message to its translation, built into
  the binary.
- **Language choice**: the code in effect for one run, and where it came from (flag, variable,
  config, OS, default).
- **UI language setting**: the install-wide language in the config, shared by the CLI, the web UI's
  default and the setup wizard.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: With a supported OS locale and no other setting, `status`, `files list`, `queue list`,
  `transfers list`, `doctor` and `--help` print no English text putio-sync controls, apart from
  messages missing from that catalog.
- **SC-002**: For every command with `--json` or `--csv`, the output and exit code under each
  shipped language are byte-identical to English for the same data.
- **SC-003**: At least 90% of the CLI English catalog's messages are translated in each shipped
  language.
- **SC-004**: Each published binary, full and lite, grows by no more than 400 KiB compressed for
  the catalogs, and the English-only build by no more than 20 KiB.
- **SC-005**: Picking the language adds no noticeable start-up time: `putio-sync --version` and
  `putio-sync health` take no more than 10 ms longer than before.

## Assumptions

- Machine translation is acceptable for a first version, as in 051; native speakers can correct a
  catalog by editing its file.
- The config setting that 056 saves as the UI language does not exist yet; this feature adds it and
  056 writes it. If 056 lands first, this feature uses its setting.
- Daemon messages sent to the web UI (errors, Doctor findings, What's new, notifications) stay
  English; a later feature can reuse these catalogs for them.
- Plural forms and local number and date formats are not modelled, as in 051.
- Text from plugins, hooks, Put.io and the OS is passed through untouched.
- Translations of the project site, guides and man pages are out of
  scope.
