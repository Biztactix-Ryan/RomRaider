---
archived: false
assignee: null
claimed_at: null
claimed_by_run: null
created: '2026-10-22'
depends_on:
- US-RR-64-9
id: US-RR-32-6
points: 1
status: todo
story_id: US-RR-32
tags: []
title: Write SSM K-line timing fixture LEG-SSM-20
updated: '2026-10-22'
---

Write fixture LEG-SSM-20: doc + KAT `fixtures/kat/timing-ssm-kline.toml` with the SSM K-line LinkProfile timings: the measured values from LEG-SSM-17/18 (US-RR-64 captures) next to the `iso14230_2` P1-P4 defaults (fixtures/kat/timing-iso14230-2.toml), and the break length (legacy: 20 ms at 4800 8N1). Source: docs/legacy/ssm.md §1.5, "Fixtures to write". Hand-derived (D-024).

When written, point LEG-SSM-20 in docs/legacy/fixtures-map.toml at the file (`kat = [...]`, drop `task`).