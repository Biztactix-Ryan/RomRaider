---
acceptance_criteria:
- Every vector fixture cites its standard document, edition and section (or legacy
  file:line for the transcribed NCS vectors)
- ISO 14230 default timings (including P3min = 55 ms) are exposed as named constants
  backed by a fixture, and MF-28 references them
- Checksum reference vectors cover each sum/XOR/CRC variant planned for the checksum
  engine
- J1979 vectors include boundary values (0, max) for every included PID
created: '2026-10-19'
depends_on:
- US-RR-9
epic_id: EPIC-RR-2
id: US-RR-14
points: 5
priority: must
status: done
tags:
- m0
- testing
- fixtures
- standards
title: Standard test-vector library
updated: '2026-10-23'
---

As a developer, I want published standards turned into known-answer fixtures, so that protocol, conversion and checksum code is checked against authoritative external sources.

Contents:
- SAE J1979 OBD-II PID formulas (mode 01) with worked examples
- ISO 9141-2 / 14230 / 15765 default timing parameters (P1–P4, N_*) as named constants with document references
- published reference vectors for the sum, XOR and CRC variants used by the checksum algorithms
- NCS codec vectors transcribed as data from the legacy test file (data only, no code)

Test tiers: T2. Refs: PROJECT.md T2(d), D-024.