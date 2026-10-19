---
acceptance_criteria:
- Given two definition sources for the same synthetic ROM, the harness reports zero
  disagreements when they agree and an exact per-cell diff when they do not
- Tolerances are per unit and explicit; a zero-tolerance default applies to raw values
- Unmatched tables in either source are reported, not silently skipped
created: '2026-10-19'
depends_on:
- US-RR-9
epic_id: EPIC-RR-2
id: US-RR-15
points: 3
priority: should
status: backlog
tags:
- m0
- testing
- fixtures
title: Cross-source agreement harness
updated: '2026-10-19'
---

As a developer, I want a harness that decodes the same ROM through two independent definitions and compares the results, so that agreement between independently authored sources becomes an oracle.

- Generic over definition sources. Compares raw and real values per matched table, with a configurable table-matching map and per-unit tolerance.
- Reports disagreements as Diagnostics, so the harness is usable before the importers exist.
- First used with RomRaider XML vs XDF (E10) and ECUFlash.

Test tiers: T2. Refs: PROJECT.md T2(e).