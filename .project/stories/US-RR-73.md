---
acceptance_criteria:
- For at least 3 corpus ROMs with both RomRaider XML and XDF definitions, the cross-source
  harness reports zero disagreements on matched tables (or documented, sourced exceptions)
- Scale factors between 0.99 and 1.01 are preserved exactly (MF-20); float XDF tables
  import (MF-21)
- XDF/VDF parsing is fuzzed with no panics
created: '2026-10-19'
depends_on:
- US-RR-34
- US-RR-15
- US-RR-27
epic_id: EPIC-RR-10
id: US-RR-73
points: 8
priority: could
status: backlog
tags:
- m6
- import
- definitions
title: XDF/VDF import
updated: '2026-10-19'
---

As a user with TunerPro XDF or VDF definitions, I want them importable into the same definition model, so that I can use existing community definitions and cross-check RomRaider XML definitions.

- XDF/VDF readers producing Layered docs.
- Equations parsed into the Expr AST (no string replacement hacks).
- Float tables supported; one consistent lsbfirst/flag polarity; full REGION handling; exact scale factors (no 0.99–1.01 dropping).
- Hardened XML layer for XDF.
- Feeds the cross-source agreement harness.

Test tiers: T0, T2 (cross-source agreement), T3. Closes MF-20, MF-21.