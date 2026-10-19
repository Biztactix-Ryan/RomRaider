---
archived: false
assignee: claude
claimed_at: '2026-10-19T08:57:04.260219+00:00'
claimed_by_run: orch-2026-10-19-acf8
created: '2026-10-19'
depends_on:
- US-RR-1-5
id: US-RR-3-6
points: 2
status: done
story_id: US-RR-3
tags: []
title: Add static-mut and global-singleton check
updated: '2026-10-19'
---

xtask check-globals: scan for `static mut`, lazy_static!/once_cell/LazyLock statics holding Mutex/RefCell/atomics-as-state; allowlist file with justification. CI step plus self-test with planted samples.