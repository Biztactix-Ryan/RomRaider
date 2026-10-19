---
archived: false
assignee: null
claimed_at: null
claimed_by_run: null
created: '2026-10-19'
depends_on: []
id: US-RR-3-5
points: 2
status: todo
story_id: US-RR-3
tags: []
title: Configure workspace lints and clippy disallowed_methods
updated: '2026-10-19'
---

Workspace [lints.clippy]: unwrap_used, expect_used, panic (deny in lib crates), plus clippy.toml disallowed_methods for std::thread::sleep, Instant::now, SystemTime::now with reason text; allow only in rr-core::clock::RealClock via #[expect] with comment.