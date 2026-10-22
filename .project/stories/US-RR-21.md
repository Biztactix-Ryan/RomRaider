---
acceptance_criteria:
- Every conversion factor has a KAT fixture citing an SI/NIST or equivalent reference
  (e.g. km per mile = 1.609344 exactly)
- 'Property test: converting A->B->A round-trips within floating tolerance for every
  unit pair in the same dimension'
- Every unit string appearing in the pinned logger and ECU definition corpora resolves
  to a registry unit or produces a Warning diagnostic listing the unknown string
created: '2026-10-19'
depends_on:
- US-RR-17
epic_id: EPIC-RR-3
id: US-RR-21
points: 3
priority: must
status: done
tags:
- m1
- core
- concepts
title: Units registry (C5)
updated: '2026-10-22'
---

As a user, I want units defined once with correct conversions and display ranges, so that gauges, dyno maths, CSV headers and unit switching are consistent and correctly named.

- Unit registry: id, symbol, dimension, conversion to base unit, default display range and precision.
- Lookup by the unit strings used in legacy definitions.
- AFR and lambda fuel-type conversions as data.
- Replaces ConverterMaxMinDefaults and the Imperial/Metric string branching.

Test tiers: T0, T1, T2 (conversion factors from SI/NIST references). Closes MF-42. Refs: ARCHITECTURE.md C5.