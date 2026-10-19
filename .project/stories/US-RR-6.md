---
acceptance_criteria:
- Tests requiring the external or private corpus are reported as skipped (with the
  env var to set) when it is absent, and never counted as passed
- The fixture loader rejects a fixture file with an unknown schema version
- rr-testkit cannot be a normal (non-dev) dependency of any crate - enforced by the
  dependency-rule test
- VirtualClock helpers let a test advance time deterministically with no real sleeping
  (test runs in under 10 ms for a simulated 10 s)
created: '2026-10-19'
depends_on:
- US-RR-1
epic_id: EPIC-RR-1
id: US-RR-6
points: 5
priority: must
status: backlog
tags:
- m0
- testing
- testkit
title: rr-testkit foundation
updated: '2026-10-19'
---

As a developer, I want one shared test kit, so that every crate's tests use the same fixtures, snapshots, generators and virtual time instead of ad-hoc helpers.

Includes:
- insta snapshot helpers with redaction conventions
- a proptest strategy module (strategies added by later stories)
- a fixture loader with three tiers: public (in repo), external corpus (`RR_CORPUS_DIR`) and private (`RR_FIXTURES_DIR`); missing tiers skip with a visible message, never silently pass
- a VirtualClock harness (advance/until helpers)
- a `private-fixtures` cargo feature

dev-dependency only.

Test tiers: T0. Refs: ARCHITECTURE.md rr-testkit, D-024, D-025.