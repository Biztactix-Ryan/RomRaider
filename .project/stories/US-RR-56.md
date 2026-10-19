---
acceptance_criteria:
- Each profiled device decodes a replay fixture (built from the legacy mock data and
  protocol docs) to hand-checked values
- A device frame failing its checksum is dropped and counted, never decoded (MF-10)
- Values older than the TTL become None; unplugging a device mid-session yields None
  within TTL (EC-24, MF-26)
- Selecting a device signal in a view does not open the port; only the sampling engine
  does (MF-32); renaming a device keeps saved profiles working (MF-33)
- Every device frame parser is fuzzed with no panics
created: '2026-10-19'
depends_on:
- US-RR-54
- US-RR-25
- US-RR-28
- US-RR-33
epic_id: EPIC-RR-7
id: US-RR-56
points: 8
priority: should
status: backlog
tags:
- m3
- live
- devices
title: Wideband devices as profiles
updated: '2026-10-19'
---

As a user with a wideband O2 controller, I want my device supported by a data profile, so that adding or fixing a device doesn't need code, and stale or corrupt device data never shows as live.

- DeviceProfile: link settings, Framing (Line/Sync/Fixed), fields (offset, Encoding, Conversion, unit), staleness TTL, stable device id (not the display name).
- About 8 legacy devices as pure profiles: AEM, AEM2, MRF, TXS, APSX, ZT2, TE, 14point7. A FrameParser hook for PLX (6-bit state machine), Innovate serial and Ecotrons.
- Devices push timestamped values. Connections are ref-counted by the sampling engine, never opened as a side effect of selection.
- Innovate MTS (COM) and Phidgets deferred (D-021).

Test tiers: T0, T1, T3, T4 (replay fixtures converted from legacy mock data), T5. Closes MF-10, MF-26, MF-32, MF-33; EC-24. Refs: ARCHITECTURE.md composition examples.