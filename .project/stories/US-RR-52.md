---
acceptance_criteria:
- Across the full simulator test suite (all protocols, all faults), the simulator
  records zero write opcodes without a valid token
- A write with a token scoped to a different ECU ID or address range is refused before
  anything is sent
- A test fails if any function other than the gated write path can construct a frame
  containing write opcodes (0xB0/0xB8 for SSM)
- cargo-mutants reports zero surviving mutants in the write-gating module
created: '2026-10-19'
depends_on:
- US-RR-47
- US-RR-7
- US-RR-36
epic_id: EPIC-RR-6
id: US-RR-52
points: 5
priority: must
status: backlog
tags:
- m2
- comms
- safety
title: ECU write gating
updated: '2026-10-19'
---

As the owner of the car, I want any operation that writes to my ECU to be impossible without an explicit, scoped confirmation, so that logging software can never change my ECU's memory by accident.

- SSM write-address/write-memory (B8/B0) and ECU reset exist only behind EcuMemoryWrite/EcuReset tokens. Tokens are scoped to the ECU ID and address range.
- The engine refuses writes outside the token's scope.
- Read-only is the default for every session.
- The simulator records every write attempt.
- A static test enumerates all code paths that can emit write opcodes.

Test tiers: T5, T6, T7. Refs: SECURITY.md Authorization, D-013.