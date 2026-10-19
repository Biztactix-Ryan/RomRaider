---
archived: false
assignee: claude
claimed_at: '2026-10-19T09:28:48.893014+00:00'
claimed_by_run: orch-2026-10-19-acf8
created: '2026-10-19'
depends_on:
- US-RR-3-6
id: US-RR-3-3
points: 2
status: done
story_id: US-RR-3
tags: []
title: 'Test: A static mut or a mutable global singleton in any workspace crate fails
  CI (clippy or a grep-based check with a...'
updated: '2026-10-19'
---

Verify acceptance criterion for story US-RR-3:

> A static mut or a mutable global singleton in any workspace crate fails CI (clippy or a grep-based check with a test proving it triggers)