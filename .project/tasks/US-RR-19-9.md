---
archived: false
assignee: null
claimed_at: null
claimed_by_run: null
created: '2026-10-19'
depends_on:
- US-RR-19-7
id: US-RR-19-9
points: 2
status: todo
story_id: US-RR-19
tags: []
title: Implement float codecs with NaN/Inf/denormal flags
updated: '2026-10-19'
---

F32/F64 decode/encode; Raw carries a FloatClass flag so edit layers can refuse non-finite values.