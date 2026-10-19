---
acceptance_criteria:
- All listed flows run headless in CI on Linux and Windows
- The refused failing-checksum save flow is part of the release-blocking T6 job
- Snapshot diffs are reviewable artifacts on failure
created: '2026-10-19'
depends_on:
- US-RR-61
- US-RR-62
- US-RR-63
epic_id: EPIC-RR-8
id: US-RR-68
points: 5
priority: should
status: backlog
tags:
- m5
- ui
- testing
title: UI smoke test suite (T10)
updated: '2026-10-19'
---

As a maintainer, I want automated end-to-end UI tests, so that the desktop app's critical flows can't silently break.

egui_kittest flows:
- launch
- open a synthetic ROM
- edit, undo, redo
- save to a temp dir (with a confirmation)
- refused failing-checksum save
- start logging against the simulator
- see gauges update
- simulate a disconnect and see the missing state

Runs headless on Linux and Windows CI.

Test tiers: T10.