---
acceptance_criteria:
- 'Property test: for any acyclic graph, evaluation order respects dependencies and
  results match direct evaluation'
- A None or stale input yields None for every downstream derived signal, never 0 (EC-25)
- A derived signal depending on a wideband device signal works end to end in a simulator
  test
- Cycles produce a diagnostic naming the cycle at load time
created: '2026-10-19'
depends_on:
- US-RR-54
- US-RR-20
- US-RR-30
epic_id: EPIC-RR-7
id: US-RR-55
points: 5
priority: should
status: backlog
tags:
- m3
- live
title: Derived-signal graph
updated: '2026-10-19'
---

As a user, I want calculated parameters (e.g. injector duty, boost from MAP and baro) computed from converted values of any source, including wideband sensors, so that derived values are correct and go missing honestly when an input is missing.

- Derived signals are Expr (C4) over `[signal:unit]` refs, resolved at load into a topologically sorted graph.
- Evaluation is on converted values per SampleFrame.
- Any None or stale input gives None output.
- Cycles and missing references produce diagnostics.
- External (device) inputs are allowed.

Test tiers: T0, T1, T2. Closes MF-24 (runtime side); EC-25. Refs: ARCHITECTURE.md C4/C12.