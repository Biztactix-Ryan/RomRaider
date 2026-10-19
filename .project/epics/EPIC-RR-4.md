---
created: '2026-10-19'
id: EPIC-RR-4
points: null
priority: must
status: draft
tags:
- m1
- definitions
target_date: null
title: Definitions (rr-defs)
updated: '2026-10-19'
---

M1. Readers for RomRaider ECU-definition XML, RomRaider logger XML and our TOML profiles, all producing Layered docs. They are resolved into frozen typed Definitions, ParamSets, LinkProfiles, DeviceProfiles and ChecksumSpecs, with a cached ROM-ID index.

Success criteria:
- Hostile XML is rejected safely (fuzzed).
- Ambiguous ROM-ID matches are refused.
- Resolution meets the performance budget on large definitions.
- Resolved trees match hand-checked known-answer fixtures.
- Closes MF-18/24/31/40, EC-03/09/10.