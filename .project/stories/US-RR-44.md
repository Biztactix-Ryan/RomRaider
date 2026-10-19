---
acceptance_criteria:
- 'Property test: an operation that fails for any selected cell changes zero bytes
  (atomic all-or-nothing, T6)'
- Interpolation across a selection with a non-linear scaling produces linearly spaced
  real values (within one raw step), not linearly spaced raw values (MF-14)
- Increment by step k from value v yields exactly v+k in real space or a rejection
  - never a doubled or retried step (MF-39)
- 'Multiply and set are locale-independent: no string round-trip anywhere in the operation
  path (MF-39)'
- cargo-mutants reports zero surviving mutants in the operation and write path
created: '2026-10-19'
depends_on:
- US-RR-40
- US-RR-24
epic_id: EPIC-RR-5
id: US-RR-44
points: 8
priority: must
status: backlog
tags:
- m1
- rom
- safety
title: Table operations written once
updated: '2026-10-19'
---

As a tuner, I want set, increment, multiply and interpolate to work identically on every table type and to refuse impossible values, so that edits are predictable and can never write out-of-range or wrapped bytes.

- Operations over a Selection of cells, written once for all table kinds.
- Set and multiply work in real space via Conversion. Increment uses explicit step rules (no recursive doubling).
- Interpolate in real space, then encode.
- Every operation produces Patches through the journal (one undo group).
- Any cell that can't be represented rejects the whole operation with per-cell reasons and no partial writes.
- NaN, Inf and denormal cells refuse edits unless explicitly allowed.

Test tiers: T0, T1, T2, T6, T7. Closes MF-14, MF-39; EC-07, EC-08. Refs: D-007.