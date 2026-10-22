---
archived: false
assignee: null
claimed_at: null
claimed_by_run: null
created: '2026-10-22'
depends_on: []
id: US-RR-63-9
points: 1
status: todo
story_id: US-RR-63
tags: []
title: Write live-value formatting fixture LEG-LOG-05
updated: '2026-10-22'
---

Write fixture LEG-LOG-05 as an rr-live formatting test. Source: docs/legacy/logger.md §4.3, "Fixtures to write". Hand-computed (D-024).

- LEG-LOG-05: `format="0"` on 750.5 -> `750`, 751.5 -> `752` (HALF_EVEN); `0.00` on 14.3647 -> `14.36`; `<replace>` match on the `.`-decimal text.

When written, point LEG-LOG-05 in docs/legacy/fixtures-map.toml at the test file (`kat = [...]`, drop `task`).