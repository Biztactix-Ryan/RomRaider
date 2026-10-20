---
acceptance_criteria:
- 'Property tests: resolution is deterministic and idempotent; overriding a field
  in a child always wins over the base for every field type including signedness (MF-06)'
- Omitting one item in the middle of a list removes only that item and keeps all later
  items (MF-07)
- A cycle across files produces an Error diagnostic naming the full cycle path - never
  a stack overflow (MF-17); a diamond inheritance resolves deterministically
- A name defined in a local scope is not visible as a global base (MF-16)
- explain(field) reports the winning layer and the overridden values with provenance
- cargo-mutants reports zero surviving mutants in the resolver
created: '2026-10-19'
depends_on:
- US-RR-17
epic_id: EPIC-RR-3
id: US-RR-22
points: 8
priority: must
status: done
tags:
- m0
- core
- concepts
- safety
title: 'Layered + Resolver (C1): one inheritance engine'
updated: '2026-10-20'
---

As a definition author, I want one well-defined layering and inheritance engine behind every configurable thing, so that base/extends/override/omit behave identically for ROM definitions, quirks, checksums, devices, logger parameters and settings.

- `Layered<T>` of partial docs with explicit merge semantics per field: replace, append, deep-merge, omit.
- Cross-file `base`/`extends`; cycle detection reports the cycle path.
- Per-field provenance (which layer won) and an explain() for debugging.
- Shared bases via Arc with no deep copy.
- Scoped namespaces so local names don't leak.

Test tiers: T0, T1, T2 (hand-checked resolution trees), T7. Closes MF-06, MF-07, MF-16, MF-17; EC-04. Refs: ARCHITECTURE.md C1.