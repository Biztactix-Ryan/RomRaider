---
created: '2026-10-19'
id: EPIC-RR-1
points: null
priority: must
status: draft
tags:
- m0
- testing
- safety
- foundation
target_date: null
title: Safety & Test Foundation
updated: '2026-10-19'
---

M0. The workspace, CI gates, lints, coverage/mutation/fuzz infrastructure, rr-testkit and capability tokens. All later work stands on these. Built BEFORE feature code (D-003).

Success criteria:
- CI green on Linux x64/arm64 and Windows x64/i686.
- Every gate enforced: fmt, clippy -D warnings, cargo-deny, coverage ≥90% on safety crates, zero surviving mutants in gated modules, nightly fuzzing.
- Banned patterns fail CI: direct sleep/Instant::now, unwrap, static mut, globals.
- Capability tokens proven unforgeable by compile-fail tests.

Refs: PROJECT.md Testing Strategy (T0-T10), ARCHITECTURE.md cross-cutting standards, D-003, D-010, D-013.