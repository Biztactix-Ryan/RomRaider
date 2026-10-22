---
archived: false
assignee: null
claimed_at: null
claimed_by_run: null
created: '2026-10-22'
depends_on: []
id: US-RR-76-6
points: 1
status: todo
story_id: US-RR-76
tags: []
title: Write SSM-over-CAN KAT LEG-SSM-21
updated: '2026-10-22'
---

Write fixture LEG-SSM-21: KAT `fixtures/kat/ssm-can.toml` (low priority): CAN variant `AA`/`EA` init, `A8`/`B8` framing with a 4-byte ID, `7F` negative responses `13` and `22`; `A0`/`B0` unsupported. Source: docs/legacy/ssm.md §7, "Fixtures to write". Hand-derived (D-024). Serves US-RR-76-3.

When written, point LEG-SSM-21 in docs/legacy/fixtures-map.toml at the file (`kat = [...]`, drop `task`).