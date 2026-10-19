---
acceptance_criteria:
- Selecting cells in the 3D view selects the same cells in the grid and vice versa
  (UI test)
- Renders under both X11 and Wayland with the glow renderer in CI (headless smoke)
- Rendering choice recorded as a DECISIONS entry
created: '2026-10-19'
depends_on:
- US-RR-61
epic_id: EPIC-RR-8
id: US-RR-66
points: 8
priority: could
status: backlog
tags:
- m4
- ui
- 3d
title: 3D surface view
updated: '2026-10-19'
---

As a tuner, I want a 3D surface view of 3D tables, so that I can spot spikes and discontinuities in maps at a glance.

- egui_wgpu paint callback or three-d (decide in-story, record in DECISIONS).
- Rotate, zoom, selection synced with the grid widget, live-overlay highlight.
- Replaces Java3D/Graph3d.

Test tiers: T10.