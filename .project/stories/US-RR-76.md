---
acceptance_criteria:
- The SSM CAN codec shares the K-line codec's message semantics (no duplicated parsing
  logic - review and test reuse)
- ISO-TP multi-frame responses reassemble correctly under split/out-of-order faults
  in the simulator
- Byte-exact KATs for CAN request/response variants
created: '2026-10-19'
depends_on:
- US-RR-47
- US-RR-69
epic_id: EPIC-RR-10
id: US-RR-76
points: 8
priority: could
status: backlog
tags:
- m6
- comms
- ssm
- can
title: SSM over CAN (ISO 15765)
updated: '2026-10-19'
---

As an owner of a newer (CAN) Subaru, I want SSM over CAN supported, so that 2008+ Subarus can be logged.

- ISO-TP framing (adapter-assembled via J2534, C6 IsoTp).
- SSM CAN request/response variants reuse the SSM codec core.
- Simulator responder over CAN and the fault matrix.

Test tiers: T0, T1, T2, T5, T9.