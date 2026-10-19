---
acceptance_criteria:
- Every non-suspect stock ROM in the external corpus that has a definition resolves
  to exactly one definition; results are committed as a coverage fixture
- A ROM matching two definitions is refused with a diagnostic listing both candidates
  and no definition is applied (EC-03)
- A ROM whose size differs from the definition's declared size produces an Error diagnostic
  and editing is disabled
- Benchmarks enforce the resolve (<200 ms for 5k tables) and lookup (<5 ms) budgets
  in CI (MF-18, MF-40, EC-10)
- Resolved Definitions are immutable (no interior mutability) - compile-time and review
  check
created: '2026-10-19'
depends_on:
- US-RR-29
- US-RR-23
epic_id: EPIC-RR-4
id: US-RR-34
points: 8
priority: must
status: backlog
tags:
- m1
- definitions
- safety
title: Definition resolution and ROM-ID index
updated: '2026-10-19'
---

As a tuner, I want the right definition picked for my ROM automatically, and refused when ambiguous, so that I can never edit a ROM through the wrong map definitions.

- Resolve layered ECU definitions into a frozen typed `Definition`: tables, scalings, axes, encodings, checksum spec reference.
- Shared bases via Arc with no deep copy.
- Cached ROM-ID index: (address, bytes) Pattern → definition. Built once per definition-set version, not re-parsed per open.
- If two definitions match, refuse and list the candidates.
- Size mismatch between ROM and definition produces a diagnostic.
- Performance budget: a 5k-table definition set resolves in under 200 ms; ROM open lookup under 5 ms.

Test tiers: T0, T1, T2, perf. Closes MF-18, MF-40; EC-03, EC-10. Refs: ARCHITECTURE.md C1/C2.