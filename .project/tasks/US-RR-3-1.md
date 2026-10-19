---
archived: false
assignee: claude
claimed_at: '2026-10-19T09:24:57.643008+00:00'
claimed_by_run: orch-2026-10-19-acf8
created: '2026-10-19'
depends_on:
- US-RR-3-5
- US-RR-2-5
id: US-RR-3-1
points: 2
status: done
story_id: US-RR-3
tags: []
title: 'Test: A test crate calling std::thread::sleep or Instant::now outside RealClock
  fails clippy in CI'
updated: '2026-10-19'
---

Verify acceptance criterion for story US-RR-3:

> A test crate calling std::thread::sleep or Instant::now outside RealClock fails clippy in CI