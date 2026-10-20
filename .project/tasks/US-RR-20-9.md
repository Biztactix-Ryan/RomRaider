---
archived: false
assignee: claude
claimed_at: null
claimed_by_run: null
created: '2026-10-19'
depends_on:
- US-RR-20-6
id: US-RR-20-9
points: 1
status: done
story_id: US-RR-20
tags: []
title: Add expr fuzz target
updated: '2026-10-20'
---

Add a cargo-fuzz target `expr` in fuzz/ (picked up by fuzz.yml via `cargo xtask fuzz list`) that parses arbitrary input with rr_core::expr (default Limits) and, when it parses, evaluates it with arbitrary x/params; no panics, hangs or unbounded allocation. Seed corpus from the KAT formulas; regression inputs under fuzz/regressions.