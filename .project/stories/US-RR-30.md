---
acceptance_criteria:
- Given SSM init bytes captured from a real ECU (fixture), resolve() yields exactly
  the parameters whose capability bits are set - hand-checked against the bit map
- A derived parameter with a missing dependency produces a diagnostic naming the dependency
  and stays visible as unavailable (MF-24)
- Every parameter used by MAF/injector/dyno collectors has a role assigned for SSM
  (MF-31)
- Logger definition parsing is fuzzed with no panics
created: '2026-10-19'
depends_on:
- US-RR-27
- US-RR-22
- US-RR-24
epic_id: EPIC-RR-4
id: US-RR-30
points: 8
priority: must
status: backlog
tags:
- m1
- definitions
- logger
title: Logger-definition reader to ParamSet
updated: '2026-10-19'
---

As a user logging my car, I want the RomRaider logger definition file to load into a typed parameter set for my exact protocol and ECU, so that I see every parameter my ECU supports and nothing silently disappears.

- Reads the full logger XML (user-supplied; not in repo) once, with no parse-time filtering.
- resolve(protocol, ecu_init_bytes) -> ParamSet, with supported/unsupported decided from SSM ECU capability bits.
- Derived parameter definitions become a dependency graph spec.
- Missing dependencies produce diagnostics, not silent removal.
- Roles (C13) assigned for engine_speed, maf_volts, etc.

Test tiers: T0, T2 (hand-checked conversions from the definition corpus), T3. Closes MF-24, MF-31 (roles). Refs: ARCHITECTURE.md C12/C13.