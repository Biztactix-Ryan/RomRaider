---
archived: false
assignee: null
claimed_at: null
claimed_by_run: null
created: '2026-10-22'
depends_on: []
id: US-RR-61-9
points: 1
status: todo
story_id: US-RR-61
tags: []
title: Write editor display-format fixture LEG-ENC-17
updated: '2026-10-22'
---

Write fixture LEG-ENC-17 as an editor test. Source: docs/legacy/encoding-scaling.md §8, "Fixtures to write". Hand-computed (D-024).

- LEG-ENC-17: `format="#.##"`: 2.125 -> `2.12`, 2.375 -> `2.38` (HALF_EVEN); the default format; the format never changes the stored value.

When written, point LEG-ENC-17 in docs/legacy/fixtures-map.toml at the test file (`kat = [...]`, drop `task`).