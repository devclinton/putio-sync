# Feature Specification: Anonymous usage stats (part 2 of 058)

**Feature Branch**: `058-usage-stats`

**Created**: 2026-11-22

**Status**: Implemented

**Input**: ROADMAP.md, "058 Crash reports, then anonymous usage stats", part 2.

## Context

- Nothing in putio-sync calls home. Part 1 (`specs/058-crash-reports/`) kept it that way: the
  person files crash reports themselves.
- This part is a once-a-day ping that is off by default. There is no built-in address: it needs
  `[usage_stats] enabled = true` and an `endpoint` the person (or a packager) sets.
- Outgoing HTTP already has a guard (`net_guard`): resolved addresses are checked against a
  reach level.

## User Scenarios

### US1 - See exactly what would be sent (P1)

In Settings, Advanced, **Usage stats**, and with `putio-sync usage-stats show`, the person sees
the exact JSON document, whether it is on, and when it was last sent.

### US2 - Opt in and out (P1)

A checkbox in the card, `PUT /usage-stats {enabled}` and `putio-sync usage-stats enable|disable`
change `usage_stats.enabled`. Off sends nothing, ever.

### US3 - Reset the id (P2)

**Reset id** (`POST /usage-stats/reset-id`) makes a new random install id.

### US4 - Run a receiver (P3)

`packaging/usage-stats-worker/` is a Cloudflare Worker that validates against an allow-list and
tallies per day in KV. It is optional and not deployed by this project.

## Requirements

- FR-001 Nothing is sent unless `enabled` and `endpoint` are both set.
- FR-002 The document has a fixed set of keys: schema, install id, version, install kind, OS,
  arch, features (radarr, sonarr, dlna, hooks, bundled plugins enabled), bucketed files per day,
  bucketed library size, crash report count.
- FR-003 OS and arch are normalised to a known list or `other`. Plugin names are only bundled
  ones. Nothing else comes from the config: no paths, hosts, names or tokens.
- FR-004 At most one attempt per 24 hours, with up to one hour of random delay. The attempt time
  is saved before sending, so a failure waits a day and an unsaved state sends nothing.
- FR-005 The request has a 10 second timeout, no redirects, no cookies, and goes through the
  public-address guard, so a local endpoint is refused by default.
- FR-006 The endpoint must be http(s) with a host, no userinfo, at most 2048 characters.

## Out of scope

- The 056 review-screen offer (the screen does not exist yet).
- Any built-in endpoint.
