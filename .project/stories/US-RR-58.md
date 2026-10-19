---
acceptance_criteria:
- No collector references a protocol-specific parameter ID (grep test plus a role-binding
  unit test per collector)
- Dyno power and torque match hand-calculated KAT fixtures for at least 3 vehicle
  specs with sources
- MAF and injector collectors produce identical results from an SSM replay and an
  equivalent synthetic OBD replay via role mapping
- FilterSet rejections are reported per filter with counts
created: '2026-10-19'
depends_on:
- US-RR-54
- US-RR-30
- US-RR-21
- US-RR-40
epic_id: EPIC-RR-7
id: US-RR-58
points: 8
priority: should
status: backlog
tags:
- m3
- live
- collectors
title: 'Role-based collectors: MAF, injector, dyno'
updated: '2026-10-19'
---

As a tuner, I want MAF scaling, injector and dyno tools that work on any protocol, so that they're not hard-wired to Subaru SSM parameter IDs.

- `trait Collector` plus a generic `FilterSet<RangeFilter>`.
- Collectors bind to roles (engine_speed, maf_volts, cl_ol, afr…), not IDs.
- Output is a Table (C11).
- Dyno physics as a plain module: VehicleSpec, Env, units via C5, power(sample, spec, env), car definitions via serde.

Test tiers: T0, T1, T2 (hand-calculated physics), T4. Closes MF-31, MF-42. Refs: ARCHITECTURE.md C13/C14.