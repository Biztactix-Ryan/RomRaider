---
acceptance_criteria:
- Every implemented PID decodes the J1979 vector fixtures exactly, including boundary
  values
- Supported-PID bitmap walking matches hand-checked fixtures across all ranges
- DTC clear (mode 04) is only reachable with an EcuReset token (T6)
created: '2026-10-19'
depends_on:
- US-RR-41
- US-RR-14
- US-RR-36
epic_id: EPIC-RR-10
id: US-RR-77
points: 5
priority: could
status: backlog
tags:
- m6
- comms
- obd
title: OBD-II codec (SAE J1979)
updated: '2026-10-19'
---

As a user of any OBD-II car, I want standard PIDs logged, so that the logger is useful beyond Subaru SSM.

- Mode 01 PIDs, supported-PID bitmaps, mode 03/07 DTC read (read-only; no clearing without an EcuReset token).
- Decoding driven by the J1979 standard-vector fixtures.
- Over ISO 15765 / 14230 / 9141 via existing Links.
- Simulator responder.

Test tiers: T0, T1, T2, T5.