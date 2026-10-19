---
acceptance_criteria:
- A table in docs/legacy lists all 17 sleeps with legacy file:line, classification
  (standard timing / quirk / polling artefact) and the replacement
- Every standard timing in the base profiles references a standard-vector fixture;
  every quirk rule has id, note and source
- Polling artefacts (sleep(1)/sleep(2) loops) are explicitly marked as not ported,
  with the replacement mechanism named (MF-27)
created: '2026-10-19'
depends_on:
- US-RR-12
- US-RR-14
epic_id: EPIC-RR-6
id: US-RR-32
points: 3
priority: must
status: backlog
tags:
- m2
- comms
- timing
title: 'Legacy timing audit: every sleep becomes a named timing or quirk'
updated: '2026-10-19'
---

As a developer, I want every hard-coded sleep and timeout in the Java comms code mapped to a named timing parameter or a documented quirk rule, so that nothing the old code relied on is lost and nothing is cargo-culted.

Inventory:
- the 17 sleeps in legacy io/ and logger/ecu/comms (non-test)
- J2534 P1_MAX/P3_MIN/P4_MIN config values
- WAIT_P3_MIN_ONLY flags
- read-timeout counting

For each: what it waits for, whether it's a standard timing (cite the ISO section) or an ECU/adapter quirk. Output: the base SSM and KWP LinkProfiles plus initial quirk Rules with notes and sources.

Test tiers: T2 (timings backed by the standard-vector library). Closes MF-28. Refs: D-004, legacy catalogue.