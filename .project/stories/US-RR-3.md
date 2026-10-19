---
acceptance_criteria:
- A test crate calling std::thread::sleep or Instant::now outside RealClock fails
  clippy in CI
- unwrap() or expect() in non-test library code fails clippy
- A static mut or a mutable global singleton in any workspace crate fails CI (clippy
  or a grep-based check with a test proving it triggers)
- The lint configuration is documented in CONTRIBUTING with the reason for each ban
created: '2026-10-19'
depends_on:
- US-RR-1
epic_id: EPIC-RR-1
id: US-RR-3
points: 3
priority: must
status: done
tags:
- m0
- lint
- safety
title: 'Safety lints: banned time, panics, globals'
updated: '2026-10-19'
---

As a maintainer, I want the dangerous patterns that plague the Java code banned mechanically, so that reviewers don't have to catch them by eye.

Workspace lints and clippy.toml:
- `disallowed_methods` for std::thread::sleep and Instant::now/SystemTime::now outside rr-core::clock::RealClock
- `unwrap_used` and `expect_used` denied outside tests
- no `static mut`; no lazy global singletons (lazy_static/once_cell statics holding mutable state)
- `panic` in library crates denied except in documented provably-infallible spots

Test tiers: T0. Refs: ARCHITECTURE.md Cross-cutting Standards, D-010, D-011, D-019, MF-27/34/35.