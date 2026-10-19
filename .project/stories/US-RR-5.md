---
acceptance_criteria:
- A template fuzz target runs locally with cargo +nightly fuzz run and in the nightly
  workflow
- A planted panic in a toy target is found by the nightly job and auto-filed as an
  issue
- Committed crash inputs run as ordinary regression tests in ci.yml without nightly
- Fuzz targets enforce a per-input time and memory limit so hangs and unbounded allocation
  count as failures
created: '2026-10-19'
depends_on:
- US-RR-2
epic_id: EPIC-RR-1
id: US-RR-5
points: 3
priority: must
status: backlog
tags:
- m0
- testing
- fuzz
- security
title: Fuzz infrastructure
updated: '2026-10-19'
---

As a maintainer, I want a fuzzing harness that every reader/parser plugs into, so that hostile definition packs, ROMs, traces and device streams can't crash or hang the app.

- A `fuzz/` crate (cargo-fuzz, libfuzzer-sys).
- fuzz.yml nightly: 10 min per target, cached corpus, crashes auto-filed as GitHub issues labelled `fuzz`.
- Every crash input is minimised into a committed regression test that runs in normal CI.
- Template target plus docs so later stories add targets in minutes.

Test tiers: T3. Refs: PROJECT.md Testing Strategy, SECURITY.md API Security.