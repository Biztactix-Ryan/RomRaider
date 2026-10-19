---
acceptance_criteria:
- 'Property test: a byte pattern compiled from a concrete byte string with random
  wildcards matches that string and rejects any single-bit change in a non-wildcard
  position'
- Pattern text parsing is fuzzed with no panics; malformed patterns produce diagnostics
- Matching at an offset past the end of the input returns no-match, never panics
created: '2026-10-19'
depends_on:
- US-RR-17
epic_id: EPIC-RR-3
id: US-RR-23
points: 3
priority: must
status: backlog
tags:
- m0
- core
- concepts
title: 'Pattern (C2): one matcher'
updated: '2026-10-19'
---

As a developer, I want one compiled pattern matcher reused for ROM identification, ECU-ID profile selection, quirk conditions, frame sync headers, response validation and CSV header detection, so that matching logic exists once and is tested once.

Pattern kinds:
- bytes with mask/wildcards (`80 F0 ?? 01`) at an offset
- named op
- ID prefix
- header-name set

Compiled once, cheap match, with an explanation of why a match failed.

Test tiers: T0, T1, T3. Refs: ARCHITECTURE.md C2.