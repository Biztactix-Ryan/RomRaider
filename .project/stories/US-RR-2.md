---
acceptance_criteria:
- A PR with a rustfmt violation, a clippy warning, or a failing test is blocked on
  every matrix target
- cargo-deny fails the build for a dependency with a GPL-incompatible licence or an
  open RUSTSEC advisory (demonstrated with a throwaway branch)
- A PR that leaves pending insta snapshots fails CI
- The i686-pc-windows-msvc target builds and runs the test suite
created: '2026-10-19'
depends_on:
- US-RR-1
epic_id: EPIC-RR-1
id: US-RR-2
points: 5
priority: must
status: active
tags:
- m0
- ci
- testing
title: 'CI gates: fmt, clippy, nextest matrix, cargo-deny'
updated: '2026-10-19'
---

As a maintainer, I want every push and PR gated by formatting, lints, tests on all tier-1/2 targets and supply-chain checks, so that nothing unsafe or broken merges.

ci.yml jobs:
- fmt
- clippy -D warnings
- nextest on x86_64-linux, aarch64-linux, x86_64-windows-msvc, i686-windows-msvc
- insta pending-snapshot check
- cargo-deny (licences compatible with GPL-2.0-or-later, advisories, bans, sources)

Test tiers: T0. Refs: INFRASTRUCTURE.md Build Pipeline, D-016.