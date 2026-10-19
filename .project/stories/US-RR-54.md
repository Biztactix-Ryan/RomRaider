---
acceptance_criteria:
- A parameter missing from a response appears as None in the frame and never as 0.0
  (MF-09) - replay and simulator tests
- A stalled subscriber never blocks the producer; dropped-frame counters are exposed
  and tested
- Start/stop under load 1000 times leaves no threads or channels alive (stress test,
  MF-34)
created: '2026-10-19'
depends_on:
- US-RR-41
epic_id: EPIC-RR-7
id: US-RR-54
points: 8
priority: should
status: backlog
tags:
- m3
- live
- safety
title: SampleFrame pipeline
updated: '2026-10-19'
---

As a user watching live data, I want every value to arrive timestamped and explicitly missing when it's missing, so that I never mistake a gap or a stale value for a real reading.

- `Signal { id, unit, role? }`.
- `SampleFrame { t_mono, t_wall, values: Vec<Option<f64>> }` indexed densely.
- Bounded broadcast to subscribers (UI, CSV, collectors, overlay); slow subscribers drop frames with a counter, never block the producer.
- Stop via cancellation token.
- No per-value UI dispatch.

Test tiers: T0, T1, T4, T5. Closes MF-09, MF-34. Refs: ARCHITECTURE.md C12, D-011, D-012.