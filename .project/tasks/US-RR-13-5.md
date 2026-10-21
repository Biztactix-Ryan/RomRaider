---
archived: false
assignee: claude
claimed_at: '2026-10-21T00:09:50.768919+00:00'
claimed_by_run: orch-2026-10-21-c4d9
created: '2026-10-19'
depends_on: []
id: US-RR-13-5
points: 3
status: done
story_id: US-RR-13
tags: []
title: Implement ROM-leak guard checker
updated: '2026-10-21'
---

xtask leak-guard: reject files >64KB under fixtures/public, bytes matching any corpus manifest sha256, ROM-ID patterns, and 17-char VIN patterns in traces/logs. Self-tests with planted samples.