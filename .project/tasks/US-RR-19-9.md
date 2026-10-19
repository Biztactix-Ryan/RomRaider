---
archived: false
assignee: claude
claimed_at: '2026-10-19T14:29:46.905444+00:00'
claimed_by_run: orch-2026-10-19-d2c2
created: '2026-10-19'
depends_on:
- US-RR-19-7
id: US-RR-19-9
points: 2
status: done
story_id: US-RR-19
tags: []
title: Implement float codecs with NaN/Inf/denormal flags
updated: '2026-10-20'
---

F32/F64 decode/encode; Raw carries a FloatClass flag so edit layers can refuse non-finite values.