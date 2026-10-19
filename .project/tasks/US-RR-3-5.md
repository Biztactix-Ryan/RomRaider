---
archived: false
assignee: claude
claimed_at: '2026-10-19T08:54:30.010084+00:00'
claimed_by_run: orch-2026-10-19-acf8
created: '2026-10-19'
depends_on:
- US-RR-1-5
id: US-RR-3-5
points: 2
status: done
story_id: US-RR-3
tags: []
title: Configure workspace lints and clippy disallowed_methods
updated: '2026-10-19'
---

Workspace [lints.clippy]: unwrap_used, expect_used, panic (deny in lib crates), plus clippy.toml disallowed_methods for std::thread::sleep, Instant::now, SystemTime::now with reason text; allow only in rr-core::clock::RealClock via #[expect] with comment.