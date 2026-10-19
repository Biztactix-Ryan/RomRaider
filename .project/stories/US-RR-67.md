---
acceptance_criteria:
- Replaying a trace highlights the correct table cells for each frame (deterministic
  UI snapshot at chosen timestamps)
- Learning-table values from a replay fixture display through the standard Table widget
  with no separate panel code
- Overlay goes inactive when the frame is stale or the link is down
created: '2026-10-19'
depends_on:
- US-RR-61
- US-RR-63
- US-RR-58
epic_id: EPIC-RR-8
id: US-RR-67
points: 8
priority: should
status: backlog
tags:
- m5
- ui
title: Live overlay, learning tables and collector views
updated: '2026-10-19'
---

As a tuner, I want to see where the engine is operating on my maps in real time, and to view learned values and collector results, so that I can tune from evidence.

- Live operating-point overlay on tables: subscribes to SampleFrames, numeric values only (no formatted strings).
- SSM learning-table values (and later DS2/NCS) shown through the single Table widget.
- MAF/injector/dyno collector outputs as Tables.

Test tiers: T10, T4.