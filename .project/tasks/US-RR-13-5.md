---
archived: false
assignee: null
claimed_at: null
claimed_by_run: null
created: '2026-10-19'
depends_on: []
id: US-RR-13-5
points: 3
status: todo
story_id: US-RR-13
tags: []
title: Implement ROM-leak guard checker
updated: '2026-10-19'
---

xtask leak-guard: reject files >64KB under fixtures/public, bytes matching any corpus manifest sha256, ROM-ID patterns, and 17-char VIN patterns in traces/logs. Self-tests with planted samples.