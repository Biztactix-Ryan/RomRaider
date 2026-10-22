---
acceptance_criteria:
- 'Property test: any sequence of valid frames, split at arbitrary byte boundaries
  and fed incrementally, yields exactly the original frames in order for every variant'
- Garbage before a header, two frames in one read, and a frame split across reads
  are all handled (EC-17)
- A Sync frame failing its checksum is dropped and reported, never emitted as data
  (MF-10)
- Every variant has a fuzz target with no panics or unbounded buffering
created: '2026-10-19'
depends_on:
- US-RR-23
- US-RR-18
epic_id: EPIC-RR-3
id: US-RR-25
points: 5
priority: must
status: ready
tags:
- m0
- core
- concepts
- comms
title: 'Framing (C6): one framer for ECUs and devices'
updated: '2026-10-23'
---

As a developer, I want byte streams split into frames by one tested framer, so that ECU responses and wideband device streams share framing logic, and corrupt frames are dropped consistently.

Variants:
- LengthByte{offset, overhead} (SSM)
- KwpHeader
- IdleGap (driven by Clock)
- IsoTp (adapter-assembled)
- Line{delim}
- Sync{header: Pattern, len, checksum}
- Fixed(n)

Incremental: feed bytes, get frames. Resynchronises after garbage. Checksum failures drop the frame and report a diagnostic event.

Test tiers: T0, T1, T3. Closes MF-10; EC-17. Refs: ARCHITECTURE.md C6.