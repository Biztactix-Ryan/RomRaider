---
acceptance_criteria:
- NCS codec vectors (transcribed from legacy tests) pass
- Supported-PID discovery against the simulator matches the legacy catalogue's documented
  behaviour
- No literal delays in NCS code - all waits come from the resolved profile (grep test
  plus trace timing assertion)
created: '2026-10-19'
depends_on:
- US-RR-70
- US-RR-69
epic_id: EPIC-RR-10
id: US-RR-78
points: 8
priority: could
status: backlog
tags:
- m6
- comms
- ncs
title: NCS codec (Nissan, K-line and CAN)
updated: '2026-10-19'
---

As a Nissan user, I want NCS logging supported on K-line and CAN, so that RomRaider's Nissan users are covered.

- NCS over KWP (K-line) and UDS-style over CAN (service 0x2C dynamic identifiers).
- Supported-PID discovery building an SSM-like init string as legacy does (spec from the legacy catalogue). The P3min wait comes from the profile (MF-28), not a sleep.
- NCS codec vectors from the standard-vector library.
- Simulator responder.

Test tiers: T0, T1, T2, T5.