---
archived: false
assignee: null
claimed_at: null
claimed_by_run: null
created: '2026-10-19'
depends_on: []
id: US-RR-57-6
points: 3
status: todo
story_id: US-RR-57
tags: []
title: Implement CSV writer with fixed schema and rotation
updated: '2026-10-19'
---

rr-live::csv: subscriber writing raw numbers, '.' decimal, empty for None, mono+wall time columns; rotate on schema change; fsync on rotate/close.