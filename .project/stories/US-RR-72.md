---
acceptance_criteria:
- Byte-exact KATs for each DS2 command type from the legacy catalogue
- A baud change mid-session works against the simulator, including a fault during
  the switch (EC-22)
- DS2 learning tables render through the generic Table type with no DS2-specific view
  code
created: '2026-10-19'
depends_on:
- US-RR-41
- US-RR-36
epic_id: EPIC-RR-10
id: US-RR-72
points: 8
priority: could
status: backlog
tags:
- m6
- comms
- ds2
title: DS2 codec (BMW)
updated: '2026-10-19'
---

As a BMW DS2 user, I want DS2 logging supported, so that RomRaider's BMW users aren't left behind.

- DS2 framing and commands (read address, read procedure, group params, user-selected address list).
- Baud change mid-session.
- Learning-table queries mapped to the Table concept.
- Simulator responder and fault matrix.

Test tiers: T0, T1, T2, T3, T5. Closes EC-22.