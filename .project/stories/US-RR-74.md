---
acceptance_criteria:
- The licence compatibility finding is recorded in DECISIONS.md before any reference
  code is read
- 'If pursued: SSM logging over native OP2 produces traces equivalent to the J2534
  path on the same car (HIL report)'
- The spike issues no write operations, and outcome and recommendation are recorded
  as a DECISIONS update to D-023
created: '2026-10-19'
depends_on:
- US-RR-33
- US-RR-41
epic_id: EPIC-RR-10
id: US-RR-74
points: 13
priority: could
status: backlog
tags:
- m6
- spike
- comms
- openport
title: 'Spike: native OpenPort 2.0 Link'
updated: '2026-10-19'
---

As an OpenPort 2.0 owner on any OS, I want the cable driven natively from Rust, so that I need no vendor DLL or third-party .so, and the path is fully traceable and testable.

Spike (D-023):
1. Verify the licence of the open-source Linux J2534 implementation is compatible with GPL-2.0-or-later BEFORE reading its code.
2. Implement the OP2 USB command protocol over nusb for K-line SSM logging.
3. Compare traces against the J2534 path.

Time-boxed. Read-only.

Test tiers: T5 (USB mock), T9.