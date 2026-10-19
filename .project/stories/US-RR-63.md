---
acceptance_criteria:
- Driven by the simulator with a dropped-parameter fault, the affected gauge shows
  a missing state, not 0 (egui_kittest snapshot)
- Link-health indicator changes state within one poll cycle of a simulated disconnect
- Warning thresholds persist in the user profile and round-trip (MF-41)
- 'The UI thread never blocks on the link: a stalled simulator does not freeze rendering
  (frame-time assertion)'
created: '2026-10-19'
depends_on:
- US-RR-59
- US-RR-54
- US-RR-55
epic_id: EPIC-RR-8
id: US-RR-63
points: 13
priority: should
status: backlog
tags:
- m5
- ui
- logger
title: 'Logger UI: parameters, dashboards, gauges, charts'
updated: '2026-10-19'
---

As a user logging my car, I want clear live gauges and charts that make missing or stale data obvious, so that I can trust what I'm looking at while driving or tuning.

- Parameter selection from the resolved ParamSet (unsupported shown as unavailable).
- Dashboards and gauges with unit switching via C5. Warning thresholds saved in the profile (MF-41).
- Charts via egui_plot.
- Link-health indicator. None and stale values rendered distinctly (greyed, with age), never as 0.
- The UI only reads the latest frame.
- Touch/fullscreen as a theme, not duplicate code.

Test tiers: T10, T5 (driven by the simulator). Closes MF-41. Refs: D-012.