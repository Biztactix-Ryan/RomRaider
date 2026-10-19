---
acceptance_criteria:
- Byte-exact request KATs for each service, and response parsing fuzzed
- The simulator responder passes the full fault matrix
- Default timings come from the standard-vector fixtures with no literals in codec
  code
created: '2026-10-19'
depends_on:
- US-RR-41
- US-RR-36
- US-RR-14
epic_id: EPIC-RR-10
id: US-RR-70
points: 8
priority: could
status: backlog
tags:
- m6
- comms
- kwp
title: KWP2000 codec (ISO 14230)
updated: '2026-10-19'
---

As a user of a KWP2000 ECU, I want it supported as a codec over the existing engine, so that fast-init K-line ECUs can be logged.

- KWP header framing (already in C6); fast init and 5-baud init via adapter or a serial break.
- StartCommunication, ReadDataByLocalId/Address, StopCommunication.
- Timing from the ISO 14230 standard vectors.
- Simulator responder and fault matrix.

Test tiers: T0, T1, T2, T3, T5.