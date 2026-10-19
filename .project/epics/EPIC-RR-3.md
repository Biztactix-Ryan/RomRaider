---
created: '2026-10-19'
id: EPIC-RR-3
points: null
priority: must
status: draft
tags:
- m0
- m1
- core
- concepts
target_date: null
title: Core Concepts (rr-core)
updated: '2026-10-19'
---

M0–M1. The pure concept types every other crate reuses (ARCHITECTURE.md C1–C10, C15): Diagnostic/Provenance, Clock, Encoding, Expr, Conversion, Units, Layered/Resolver, Pattern, Framing, Rule engine. No I/O, no OS dependencies.

Success criteria:
- Every concept has unit + property tests.
- Encoding, Conversion and Resolver are mutation-gated with zero survivors.
- The Expr parser and every Framing variant are fuzzed.
- Closes MF-02/04/05/06/07/10/13/15/16/17/25/42.