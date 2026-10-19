---
acceptance_criteria:
- The simulator answers SSM init with configurable ECU ID/capability bytes and serves
  read-address and read-memory from its image, verified against the SSM legacy catalogue
  fixtures
- Each fault action from the Rule set is demonstrated by a test that observes the
  fault on the wire via trace
- Every write request received is recorded with token metadata, and a helper asserts
  zero unauthorised writes
- A 10-minute simulated logging session with faults runs in under 2 seconds of real
  time on VirtualClock
created: '2026-10-19'
depends_on:
- US-RR-33
- US-RR-26
- US-RR-31
epic_id: EPIC-RR-6
id: US-RR-36
points: 8
priority: must
status: backlog
tags:
- m2
- comms
- testing
- simulator
title: ECU simulator with rule-based fault injection
updated: '2026-10-19'
---

As a developer, I want a simulated ECU that speaks real protocols, serves a real ROM/RAM image and injects faults on demand, so that every comms path, including failure paths and write attempts, is tested in CI without a car.

- SimLink implements Link (C7).
- Protocol responders are codecs in reverse (SSM first).
- Memory map backed by an Image: ROM from the corpus or synthetic, RAM synthetic with scripted value curves.
- Faults are Rules (C8) applied ECU-side: drop, delay, corrupt byte, truncate, no echo, go silent after N, garbage prefix, baud mismatch.
- Records every write attempt with its token, or the lack of one.
- Runs on VirtualClock.

Test tiers: T5 (this is the T5 platform), T0. Refs: ARCHITECTURE.md composition examples, D-005.