---
acceptance_criteria:
- Every table in every synthetic ROM decodes to the generator's manifest values (raw
  and real) via the Table API
- Two tables overlapping the same bytes with different storage widths always reflect
  the current image after any patch (property test, MF-03, EC-01)
- 3D tables report correct dimensions and compare/copy over all cells - no dummy 1x1
  arrays (MF-12)
- A table whose address range exceeds the image is disabled with an Error diagnostic
  and never panics (EC-02); editing a shared axis is visible in every dependent table
  view (EC-05)
- Every table in every non-suspect corpus ROM with a definition decodes without Error
  diagnostics (results snapshot)
created: '2026-10-19'
depends_on:
- US-RR-31
- US-RR-34
- US-RR-10
epic_id: EPIC-RR-5
id: US-RR-40
points: 8
priority: must
status: backlog
tags:
- m1
- rom
- safety
title: N-D Table as views over the image (C11)
updated: '2026-10-19'
---

As a tuner, I want every table (1D/2D/3D/switch/bitwise) to be one type that reads its cells straight from the ROM bytes, so that overlapping tables, shared axes and table copies can never disagree with the actual ROM contents.

- `Table { axes: Vec<Axis>, cells: Grid<CellRef>, kind }`. CellRef = address + Encoding + Conversion. No cached values.
- Axes can be shared between tables.
- A table range past the image end is disabled with a diagnostic.
- Data layouts (row/column order, swapXY/flip) as data.
- Read API only in this story; writes come in the operations story.

Test tiers: T0, T1, T2 (synthetic ROMs + corpus decode), T7. Closes MF-03, MF-12; EC-01, EC-02, EC-05. Refs: ARCHITECTURE.md C11, D-007.