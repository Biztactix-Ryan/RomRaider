---
archived: false
assignee: null
claimed_at: null
claimed_by_run: null
created: '2026-10-19'
depends_on: []
id: US-RR-45-7
points: 2
status: todo
story_id: US-RR-45
tags: []
title: Implement atomic write with fsync, rename and .bak
updated: '2026-10-19'
---

Temp file in destination dir, fsync file+dir, rename; previous file kept as .bak; default output path never the source.