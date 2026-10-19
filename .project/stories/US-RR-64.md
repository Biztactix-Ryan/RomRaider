---
acceptance_criteria:
- The Liberty's ECU ID and ROM ID are recorded in the HIL report and in VISION.md
  (replacing TBD)
- Each scenario (idle, drive, unplug/replug, ignition off, engine start) has a scrubbed
  trace committed as a replay fixture and a passing replay test
- The HIL run's simulator-trace equivalents expose no behaviour the simulator did
  not model; any gap becomes a new simulator rule or fixture
- The HIL harness refuses to run any write operation (asserted in its own test)
created: '2026-10-19'
depends_on:
- US-RR-60
- US-RR-53
epic_id: EPIC-RR-6
id: US-RR-64
points: 5
priority: must
status: backlog
tags:
- m2
- hil
- ssm
- kkl
title: Hardware-in-loop read-only suite on the 2004 Liberty
updated: '2026-10-19'
---

As the owner of the target car, I want a repeatable read-only test session on my Liberty via the KKL cable, so that real hardware behaviour is verified and captured as regression fixtures.

HIL script (manual, guided by `romraider-cli hil`):
- identify ECU (records ECU ID / ROM ID)
- log a standard parameter set for N minutes at idle and on a short drive
- unplug and replug the cable mid-session
- ignition off mid-session
- engine start while connected

Every session is captured as .rrtrace, scrubbed, and committed as replay fixtures. HIL report template in docs. Read-only only: no write tests until a bench ECU exists.

Test tiers: T9, T4 (resulting fixtures). Refs: INFRASTRUCTURE.md HIL, VISION.md target.