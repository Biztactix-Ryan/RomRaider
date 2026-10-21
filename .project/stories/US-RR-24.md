---
acceptance_criteria:
- 'Property test: for every invertible conversion and every raw value r, to_raw(to_real(r))
  == r exactly for linear forms and within one raw step for solved forms'
- Decreasing scalings report correct min/max display ranges (MF-13) - KAT fixtures
  with hand-calculated ranges
- A non-monotonic conversion with no to_raw is rejected for editing with a diagnostic;
  4-byte storage inverse never overflows (MF-05)
- A target real value outside the representable range, or hitting a division-by-zero
  region, is rejected with a reason and no bytes change (EC-08, T6)
- The same input always yields the same output regardless of call order (no cache-dependent
  results) - property test with shuffled call sequences
- cargo-mutants reports zero surviving mutants in the conversion module
created: '2026-10-19'
depends_on:
- US-RR-20
- US-RR-19
epic_id: EPIC-RR-3
id: US-RR-24
points: 8
priority: must
status: done
tags:
- m1
- core
- concepts
- safety
title: 'Conversion (C4b): exact inverses, safe ranges'
updated: '2026-10-21'
---

As a tuner, I want the value I type to be written as exactly the right raw bytes, or refused, so that a scaling bug can never silently write a wrong value into my ROM.

`Conversion = Linear{m,b} | Expr{to_real, to_raw?} | Lut`, with unit and display format.
- Detect linear forms in Expr and use the exact algebraic inverse.
- With no to_raw: a bounded numeric solve over the encoding's valid raw range. It must be monotonic over that range or it's an error.
- Range = min/max of f over the raw range: endpoints plus extrema, correct for decreasing functions.
- An out-of-range or non-invertible target is rejected with a reason, never wrapped or clamped.
- Deterministic, no cache-dependent results.

Test tiers: T0, T1, T2, T6, T7. Closes MF-05, MF-13; EC-08. Refs: ARCHITECTURE.md C4.