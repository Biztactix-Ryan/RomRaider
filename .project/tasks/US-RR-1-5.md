---
archived: false
assignee: claude
claimed_at: '2026-10-19T08:39:15.574690+00:00'
claimed_by_run: orch-2026-10-19-acf8
created: '2026-10-19'
depends_on: []
id: US-RR-1-5
points: 3
status: done
story_id: US-RR-1
tags: []
title: Scaffold Cargo workspace and crates
updated: '2026-10-19'
---

Create root Cargo.toml (workspace, resolver 3, edition 2024), crates/rr-core, rr-defs, rr-rom, rr-link, rr-comms, rr-live, rr-testkit, rr-cli, rr-app and xtask with empty lib/main. Add rust-toolchain.toml, workspace [lints] table, commit Cargo.lock, ignore target/.