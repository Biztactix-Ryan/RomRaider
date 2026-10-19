---
acceptance_criteria:
- 'Property test: applying diff(a,b) to a yields b exactly, and diff(a,a) is empty'
- Diff reports raw and real differences per table and is linear time (benchmark on
  1 MB images)
- Clipboard copy then paste round-trips every table type exactly, under any process
  locale (test runs with de_DE and fr_FR locales)
- Pasting legacy-format clipboard text produces the same values as the legacy catalogue
  fixture specifies
created: '2026-10-19'
depends_on:
- US-RR-40
- US-RR-12
epic_id: EPIC-RR-5
id: US-RR-46
points: 5
priority: must
status: backlog
tags:
- m1
- rom
title: Diff via patches and locale-safe clipboard
updated: '2026-10-19'
---

As a tuner, I want to compare two ROMs and copy/paste tables reliably, so that I can see exactly what changed between tunes and move values between tables without locale or format surprises.

- `diff(image_a, image_b, definition)` gives Patches grouped by table, comparing both raw and real values. O(n) over bytes.
- "Apply diff" goes through the journal.
- Clipboard codec: tab-separated with a header line, `.` decimal always, round-trips with the legacy RomRaider clipboard format for interop (spec from the legacy catalogue).

Test tiers: T0, T1, T2. Closes MF-22. Refs: ARCHITECTURE.md C10.