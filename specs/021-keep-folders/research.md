# Research: Keep folders out of empty-folder cleanup

## R1. Where the keep decision is made

- **Decision**: The walk in `putio-sync-client` asks a callback (`KeepFn`) for each folder;
  `putio-sync-core::cleanup` builds the callback from the `RuleSet` and the keep patterns.
- **Rationale**: `client` must not depend on `core` (Constitution I), but the walk is the
  one place that knows each folder's path and whether it is empty. A callback keeps the
  rules in core and the walk in client.
- **Alternatives**: pass a set of kept folder ids (would need a separate path → id lookup
  walk); move the walk into core (large move for a small feature).

## R2. Keep as a folder rule setting

- **Decision**: `FolderRule.keep: Option<bool>`, resolved into `Effective.keep:
  Sourced<bool>` like `skip`.
- **Rationale**: inheritance, "No" overriding a parent, the rule card's source labels, the
  editor's Inherit/Yes/No select and the Settings summary all come from the existing FLAGS
  machinery.

## R3. Matching keep patterns against folders

- **Decision**: Feature 020's `Pattern::matches(name, path)` with the folder's name and its
  path from the root, segments cleaned with `rules::sanitize_name` as rule paths are. The
  folder and each ancestor path are tried, so a pattern keeps what is inside a match.
- **Rationale**: one pattern language for the whole app; ancestor matching gives the same
  "and everything below" meaning as a rule.
- **Alternatives**: one OR-joined pattern string (harder to read and edit than a list).

## R4. Reporting kept folders

- **Decision**: `EmptyFolderReport.kept: Vec<KeptFolder { id, path, reason }>` for every
  empty folder the walk kept (not the root), plus `kept_by_user`. The existing counts stay.
- **Rationale**: the preview and CLI need the reason per folder; keeping the counts avoids
  breaking the API, CLI and doctor callers.
- **Reason text**: `protected`, `too new`, `kept by the rule on Movies`, `matches keep
  pattern "Season*"`.

## R5. Applying changes without a restart

- **Decision**: rule saves (`routes/system/rules.rs::apply`) also call
  `state.cleanup.configure`; cleanup settings saves already do.
