---
acceptance_criteria:
- Every distinct formula in the pinned ECU and logger definition corpora parses, and
  a sample of at least 50 has hand-checked known-answer evaluations
- JEP operator precedence and ^ semantics match documented JEP behaviour via KAT fixtures
  citing the JEP documentation
- The expr fuzz target runs nightly with no panics, hangs or unbounded allocation;
  depth and size limits produce diagnostics
- Evaluation of a parsed AST performs no allocation and no locking (benchmark + test),
  fixing the global synchronized cache of MF-25
- cargo-mutants reports zero surviving mutants in the evaluator
created: '2026-10-19'
depends_on:
- US-RR-17
- US-RR-5
epic_id: EPIC-RR-3
id: US-RR-20
points: 8
priority: must
status: done
tags:
- m0
- core
- concepts
- safety
- security
title: 'Expr parser (C4a): JEP-compatible, parsed once'
updated: '2026-10-21'
---

As a definition author, I want existing RomRaider formulas to work unchanged and be parsed once into a typed AST, so that scalings and logger conversions evaluate fast, identically everywhere, and safely.

In-house Pratt parser covering the JEP subset used by real definitions:
- operators, precedence, `^` semantics, standard functions, BitWise(op,x,mask)
- native `& | << >>`; `x` and `[param:unit]` refs
- limits on depth, node count and literal size
- pure evaluation: no I/O, no state
- one dialect only, not two

Test tiers: T0, T1, T2 (formulas harvested from the definition corpus), T3, T7. Closes MF-25. Refs: ARCHITECTURE.md C4, D-008, SECURITY.md Formulas.