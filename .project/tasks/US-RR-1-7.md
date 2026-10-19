---
archived: false
assignee: null
claimed_at: null
claimed_by_run: null
created: '2026-10-19'
depends_on:
- US-RR-1-5
id: US-RR-1-7
points: 3
status: todo
story_id: US-RR-1
tags: []
title: Implement dependency-rule checker test
updated: '2026-10-19'
---

tests/architecture.rs (workspace-level, in xtask or rr-testkit dev) using cargo_metadata: rr-core has no workspace deps; only rr-link/rr-cli/rr-app may depend on serialport/libloading/nusb/winreg; rr-testkit only as dev-dependency. Allowlist table in one place.