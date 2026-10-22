---
acceptance_criteria:
- A rule without id, note or source is rejected at load with a diagnostic
- Layer precedence (protocol < adapter < ECU family < ECU ID) is honoured for conflicting
  actions - property test over random layer stacks
- disable-one and disable-all remove exactly the targeted rules, verified by trace
  output
- Applying rules to an op with no matching rules costs a single table lookup (benchmark
  asserts no scan)
- Unknown hook names are a load error, not a runtime surprise
- cargo-mutants reports zero surviving mutants in rule flattening and application
created: '2026-10-19'
depends_on:
- US-RR-23
- US-RR-18
- US-RR-22
epic_id: EPIC-RR-3
id: US-RR-26
points: 8
priority: must
status: ready
tags:
- m0
- core
- concepts
- comms
- safety
title: 'Rule engine (C8): quirks and faults as data'
updated: '2026-10-23'
---

As a definition author, I want ECU and adapter quirks expressed as declarative rules with a required explanation, so that oddities like "this ECU needs 60 ms after the ID request" never become unexplained sleeps in protocol code.

- Rule { id, when: Pattern + phase + prev_op, do: [Action], note, source }.
- Fixed action set: delay_before, delay_after, timeout, retry(n), max_items, drop_leading, ignore_checksum, reinit_after, hook(name).
- Rules are flattened per op at resolve time into a lookup table.
- disable-one and disable-all switches.
- Every application is reported to a trace sink.
- The same engine is reused for simulator fault injection (D-005).

Test tiers: T0, T1, T7. Closes EC-23. Refs: ARCHITECTURE.md C8, D-004, D-005.