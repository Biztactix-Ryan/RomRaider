---
acceptance_criteria:
- Every definition file in the pinned definition corpus loads; any Error diagnostic
  is either fixed or recorded as a known-bad fixture with a reason
- 'Each table type has a hand-checked KAT fixture: minimal XML in, expected typed
  layered doc out'
- Unknown elements or attributes produce Warning diagnostics naming them with line:col
  (snapshot tested)
- The reader never panics on any fuzzed definition input
created: '2026-10-19'
depends_on:
- US-RR-27
- US-RR-22
- US-RR-19
- US-RR-24
epic_id: EPIC-RR-4
id: US-RR-29
points: 8
priority: must
status: done
tags:
- m1
- definitions
title: RomRaider ECU-definition reader
updated: '2026-10-21'
---

As a tuner, I want my existing RomRaider ECU-definition XML files to load unchanged, so that I can switch to the new app without rewriting definitions.

Reads `<rom>`, `<romid>`, `<table>` (all types: 1D/2D/3D/switch/bitwise), `<scaling>`, axes, `base` inheritance and `omit` into Layered docs (C1), with Encoding (C3) and Conversion (C4) fields typed at read time. Unknown elements and attributes produce Warning diagnostics listing them, never a silent drop. Dead legacy forms ("4"/"5" axis types, unused DTD schema) are documented in the legacy catalogue.

Test tiers: T0, T2 (hand-checked per-element fixtures), T3. Refs: legacy catalogue, MF-23.