---
archived: false
assignee: claude
claimed_at: '2026-10-19T08:41:24.837703+00:00'
claimed_by_run: orch-2026-10-19-acf8
created: '2026-10-19'
depends_on:
- US-RR-1-5
id: US-RR-1-6
points: 1
status: done
story_id: US-RR-1
tags: []
title: Add romraider-cli and romraider-rs binary stubs with --version
updated: '2026-10-19'
---

rr-cli: [[bin]] name=romraider-cli using clap with --version from CARGO_PKG_VERSION. rr-app: [[bin]] name=romraider-rs printing --version (no UI yet).