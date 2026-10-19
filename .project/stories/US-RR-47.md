---
acceptance_criteria:
- Request builders produce byte-exact frames for init, read-address, read-memory,
  write-address and write-memory per KAT fixtures (with checksums)
- Response parser rejects bad header, length or checksum with a diagnostic, and the
  fuzz target finds no panics
- Requesting more addresses than the per-request limit splits into multiple requests
  and reassembles values in order (EC-21)
- Two concurrent simulated sessions with different ECUs do not interfere (no shared
  codec state, MF-29)
- A full simulated logging loop (init, identify, poll 20 params) passes against the
  simulator with and without faults
created: '2026-10-19'
depends_on:
- US-RR-41
- US-RR-36
- US-RR-12
epic_id: EPIC-RR-6
id: US-RR-47
points: 8
priority: must
status: backlog
tags:
- m2
- comms
- ssm
title: SSM codec over K-line
updated: '2026-10-19'
---

As the owner of a 2004 AU Liberty, I want the Subaru SSM protocol implemented exactly, so that my ECU is identified correctly and every parameter read is decoded from the right bytes.

- Stateless codec (no static module state, MF-29).
- Init request (0xBF) and response parsing: ECU ID, ROM ID and capability bit map.
- Read-address (0xA8) with multi-address batching up to the ECU's per-request limit (Rule max_items).
- Read-memory (0xA0).
- Response validation: header, length, checksum.
- Write-address (0xB8) and write-memory (0xB0) request builders exist, but only reachable through write gating.
- Simulator responder for the same codec.

Test tiers: T0, T1, T2 (legacy catalogue SSM fixtures + real init capture), T3, T4, T5. Closes MF-29; EC-21. Refs: legacy/docs/ssm_info.txt, legacy catalogue SSM page.