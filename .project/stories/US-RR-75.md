---
acceptance_criteria:
- For at least 3 corpus ROMs, ECUFlash vs RomRaider XML cross-source agreement reports
  zero disagreements on matched tables (or documented exceptions)
- ECUFlash include/inheritance resolves through the shared Resolver with cycle diagnostics
- ECUFlash XML parsing is fuzzed with no panics
created: '2026-10-19'
depends_on:
- US-RR-34
- US-RR-15
- US-RR-27
epic_id: EPIC-RR-10
id: US-RR-75
points: 5
priority: could
status: backlog
tags:
- m6
- import
- definitions
title: ECUFlash XML import
updated: '2026-10-19'
---

As a user with ECUFlash definitions, I want them importable, so that the larger ECUFlash definition ecosystem works, and it acts as a second independent source for cross-checking.

- ECUFlash XML reader producing Layered docs (include/inheritance semantics mapped to C1), through the hardened XML layer.
- Feeds the cross-source harness.

Test tiers: T0, T2, T3.