---
archived: false
assignee: null
claimed_at: null
claimed_by_run: null
created: '2026-10-22'
depends_on: []
id: US-RR-55-7
points: 1
status: todo
story_id: US-RR-55
tags: []
title: Write derived-signal value KAT LEG-LOG-10
updated: '2026-10-22'
---

Write fixture LEG-LOG-10: KAT `fixtures/kat/logger-derived.toml`. Source: docs/legacy/logger.md §7.3, "Fixtures to write". Hand-computed (D-024).

- LEG-LOG-10: `(P12*60)/P8` with P12 = 30.00 g/s, P8 = 3000 rpm -> 0.60 g/rev; `(P8*[P21:ms])/1200` with 3000, 10 -> 25.00; `[P7:psi]-[P24:psi]` with raw 99 and 100 -> (99-100) x 37/255 = -0.15 psi (-0.1451).

When written, point LEG-LOG-10 in docs/legacy/fixtures-map.toml at the file (`kat = [...]`, drop `task`).