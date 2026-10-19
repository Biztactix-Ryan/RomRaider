---
acceptance_criteria:
- Diagnostic codes are unique and stable - a test fails if a code is reused or removed
  without a deprecation entry
- Provenance renders as file:line:col for text sources and as an offset for binary
  sources, with snapshot tests
- Severity Error diagnostics make the owning load fail closed (the caller cannot get
  a usable T without explicitly accepting errors)
created: '2026-10-19'
depends_on:
- US-RR-1
epic_id: EPIC-RR-3
id: US-RR-17
points: 3
priority: must
status: backlog
tags:
- m0
- core
- concepts
title: Diagnostic and Provenance (C15)
updated: '2026-10-19'
---

As a developer, I want one Diagnostic type, collected rather than thrown, with provenance pointing at the exact source location, so that every load or resolve problem is visible, testable and never shows up as a popup or a swallowed exception.

- `Diagnostic { severity, code, message, provenance }`.
- Provenance: file, line/column or byte offset, layer and rule id.
- `Diagnostics` collector; loaders return `(T, Vec<Diagnostic>)`.
- Stable diagnostic codes, documented.

Test tiers: T0, T1. Refs: ARCHITECTURE.md C15, D-019, MF-23/40.