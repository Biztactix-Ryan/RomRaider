---
acceptance_criteria:
- 'Property test: frames -> CSV -> playback reproduces identical frames (values, None
  gaps and timestamps)'
- A parameter-set change mid-session rotates to a new file and the header is never
  rewritten mid-file (MF-30, EC-27)
- Output is byte-identical under de_DE, fr_FR and en_AU locales
- Importer KAT fixtures for legacy RomRaider, Cobb, AEM and OP2 headers, including
  comma-decimal legacy files (EC-28)
- A wall-clock jump during logging keeps the monotonic column strictly increasing
  (EC-26)
created: '2026-10-19'
depends_on:
- US-RR-54
- US-RR-23
epic_id: EPIC-RR-7
id: US-RR-57
points: 8
priority: should
status: backlog
tags:
- m3
- live
- csv
title: CSV writer, importer and playback producer
updated: '2026-10-19'
---

As a user, I want logs that are exact, portable and replayable, so that I can analyse sessions anywhere and replay them through the app as if live.

- Writer: subscribes to frames. Schema fixed at file start (rotate to a new file on schema change), raw numbers, `.` decimal, RFC 4180. Monotonic and wall time columns. Empty cell for None. fsync on rotate and close.
- Importer: our CSV, legacy RomRaider CSV, Cobb/AEM/OP2 header detection via Pattern (C2).
- Playback producer: CSV → SampleFrames on Clock.

Test tiers: T0, T1, T2. Closes MF-30; EC-26, EC-27, EC-28. Refs: ARCHITECTURE.md Contracts.