---
archived: false
assignee: null
claimed_at: null
claimed_by_run: null
created: '2026-10-19'
depends_on: []
id: US-RR-3-6
points: 2
status: todo
story_id: US-RR-3
tags: []
title: Add static-mut and global-singleton check
updated: '2026-10-19'
---

xtask check-globals: scan for `static mut`, lazy_static!/once_cell/LazyLock statics holding Mutex/RefCell/atomics-as-state; allowlist file with justification. CI step plus self-test with planted samples.