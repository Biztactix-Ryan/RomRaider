---
acceptance_criteria:
- Every table kind, including bitwise switches, renders in the widget (UI smoke test
  per kind, MF-38)
- Edit then undo then redo in the UI produces the same bytes as the equivalent core
  operations (egui_kittest)
- A rejected edit shows per-cell reasons and leaves the ROM unchanged
- A 1 MB ROM with 2000 tables opens and the tree is interactive within 1 second on
  CI hardware
created: '2026-10-19'
depends_on:
- US-RR-59
- US-RR-44
- US-RR-46
epic_id: EPIC-RR-8
id: US-RR-61
points: 13
priority: should
status: backlog
tags:
- m4
- ui
title: ROM tree, N-D table widget and undo/redo UI
updated: '2026-10-19'
---

As a tuner, I want to browse a ROM's tables and edit them in a fast grid with full undo/redo, so that map editing is comfortable and every change is reversible.

- ROM/definition tree with user-level filtering.
- One table widget for every Table kind: 1D/2D/3D/switch/bitwise, exhaustive match (MF-38).
- Selection ops call the core operations.
- Modified cells highlighted from the journal; per-cell rejection reasons shown.
- Compare overlay from diff; clipboard via the core codec.

Test tiers: T0, T10. Closes MF-38. Refs: D-007.