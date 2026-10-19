---
acceptance_criteria:
- Each listed inheritance case has a KAT fixture with a stated hand-calc source and
  passes
- Resolved-tree snapshots for 20 real corpus definitions are committed with a review
  note and any change requires an explanation
- Each MF entry listed is referenced from the fixture that proves it, and the ledger
  links back to the fixture
created: '2026-10-19'
depends_on:
- US-RR-34
- US-RR-9
epic_id: EPIC-RR-4
id: US-RR-39
points: 5
priority: must
status: backlog
tags:
- m1
- definitions
- testing
title: Resolved-definition known-answer suite
updated: '2026-10-19'
---

As a developer, I want hand-checked expected resolution results for the tricky inheritance cases, so that definition resolution is proven correct independently of the code, including the cases the Java got wrong.

Fixtures (small, hand-authored definition sets + expected resolved trees):
- multi-level base
- cross-file base
- omit in the middle of a list
- child overriding signedness, storage and endianness
- table-local vs global scalings
- shared axes
- diamond inheritance
- a cycle (expected diagnostic)
- ambiguous ROM ID

Plus a snapshot of resolved trees for a sample of 20 real corpus definitions, reviewed by hand once.

Test tiers: T2. Closes (proves) MF-06/07/16/17/18 at the definitions layer; EC-03/04. Refs: D-024.