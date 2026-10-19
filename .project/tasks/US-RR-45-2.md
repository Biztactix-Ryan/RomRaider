---
archived: false
assignee: null
claimed_at: null
claimed_by_run: null
created: '2026-10-19'
depends_on: []
id: US-RR-45-2
points: null
status: todo
story_id: US-RR-45
tags: []
title: 'Test: A simulated crash at every step of the save (fault injection between
  write, fsync and rename) leaves either the...'
updated: '2026-10-19'
---

Verify acceptance criterion for story US-RR-45:

> A simulated crash at every step of the save (fault injection between write, fsync and rename) leaves either the old file or the new file intact, never a partial one, and the .bak present (EC-13)