---
archived: false
assignee: null
claimed_at: null
claimed_by_run: null
created: '2026-10-22'
depends_on: []
id: US-RR-58-8
points: 1
status: todo
story_id: US-RR-58
tags: []
title: Write dyno-collector legacy CSV import fixture LEG-CSV-12
updated: '2026-10-22'
---

Write fixture LEG-CSV-12: an importer test (dyno collector input). Source: docs/legacy/csv.md §11.2, "Fixtures to write". Hand-derived (D-024).

- LEG-CSV-12: a legacy log with `Engine Speed (rpm)`, `Throttle Opening Angle (%)` and `Vehicle Speed (mph)` columns imported for the dyno collector by Role, not by header substring (UL-CSV-01, MF-31).

Uses the US-RR-57 importer. When written, point LEG-CSV-12 in docs/legacy/fixtures-map.toml at the test file (`kat = [...]`, drop `task`).