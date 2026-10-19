---
archived: false
assignee: claude
claimed_at: '2026-10-19T08:42:38.847847+00:00'
claimed_by_run: orch-2026-10-19-acf8
created: '2026-10-19'
depends_on:
- US-RR-1-5
id: US-RR-1-7
points: 3
status: done
story_id: US-RR-1
tags: []
title: Implement dependency-rule checker test
updated: '2026-10-19'
---

tests/architecture.rs (workspace-level, in xtask or rr-testkit dev) using cargo_metadata: rr-core has no workspace deps; only rr-link/rr-cli/rr-app may depend on serialport/libloading/nusb/winreg; rr-testkit only as dev-dependency. Allowlist table in one place.