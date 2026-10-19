---
acceptance_criteria:
- The suite runs in CI over the cached external corpus and fails on any regression
  in the committed coverage report
- Every non-suspect corpus ROM with a definition passes all listed invariants; any
  exception is an explicit, reasoned entry in the coverage report
- The two non-standard-size images are classified (e.g. CRLF-mangled vs trailing data)
  with evidence, not just skipped
- Randomised edit-save-reopen runs at least 1000 iterations across the corpus per
  CI run with a fixed seed recorded for reproduction
created: '2026-10-19'
depends_on:
- US-RR-35
- US-RR-16
- US-RR-10
- US-RR-40
- US-RR-45
epic_id: EPIC-RR-5
id: US-RR-51
points: 8
priority: must
status: backlog
tags:
- m1
- testing
- corpus
- safety
title: 'Corpus suite: every stock ROM, every table, every invariant'
updated: '2026-10-19'
---

As a maintainer, I want one suite that runs the full ROM core over every stock ROM and synthetic ROM, so that real-world correctness is continuously proven, not sampled.

For each corpus ROM with a definition:
- identify
- decode every table without Error diagnostics
- checksum validate passes
- fix-checksums is a no-op
- byte-flip then fix only touches checksum bytes
- random valid edits, then safe-save, then re-open validates
- suspect images classified explicitly

For each synthetic ROM: round-trip through the reference decoder. Produces a coverage report: ROM IDs × pass/fail × reason.

Test tiers: T2, T6. Refs: D-024, D-025.