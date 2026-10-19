---
archived: false
assignee: null
claimed_at: null
claimed_by_run: null
created: '2026-10-19'
depends_on:
- US-RR-25-5
id: US-RR-25-7
points: 3
status: todo
story_id: US-RR-25
tags: []
title: Implement Sync (checksum+resync), IdleGap and IsoTp framers
updated: '2026-10-19'
---

Sync uses Pattern header and checksum, drops bad frames with an event; IdleGap uses Clock deadlines; IsoTp passthrough for adapter-assembled frames.