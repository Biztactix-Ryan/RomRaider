---
archived: false
assignee: claude
claimed_at: '2026-10-19T11:10:23.776127+00:00'
claimed_by_run: orch-2026-10-19-d2c2
created: '2026-10-19'
depends_on:
- US-RR-17-4
id: US-RR-17-5
points: 2
status: done
story_id: US-RR-17
tags: []
title: Implement Diagnostics collector and fail-closed Loaded<T>
updated: '2026-10-19'
---

Collector API; Loaded<T> that withholds T when Error diagnostics exist unless caller calls accept_errors() explicitly.