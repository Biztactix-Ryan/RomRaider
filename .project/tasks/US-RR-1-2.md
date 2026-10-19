---
archived: false
assignee: claude
claimed_at: '2026-10-19T09:18:01.260752+00:00'
claimed_by_run: orch-2026-10-19-acf8
created: '2026-10-19'
depends_on:
- US-RR-1-7
id: US-RR-1-2
points: 1
status: done
story_id: US-RR-1
tags: []
title: 'Test: A test reading cargo metadata fails if rr-core depends on any workspace
  crate or on an OS/IO crate (serialport,...'
updated: '2026-10-19'
---

Verify acceptance criterion for story US-RR-1:

> A test reading cargo metadata fails if rr-core depends on any workspace crate or on an OS/IO crate (serialport, libloading, nusb, std::fs-using deps)