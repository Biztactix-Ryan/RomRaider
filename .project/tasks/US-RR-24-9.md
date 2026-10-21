---
archived: false
assignee: claude
claimed_at: null
claimed_by_run: null
created: '2026-10-19'
depends_on:
- US-RR-24-7
id: US-RR-24-9
points: 3
status: done
story_id: US-RR-24
tags: []
title: Implement bounded monotonic numeric solve
updated: '2026-10-21'
---

When no to_raw: verify monotonic over the encoding's raw range (sampling + endpoint checks), then bisect on integer raw space; non-monotonic => error. Overflow-safe for 4-byte storage.