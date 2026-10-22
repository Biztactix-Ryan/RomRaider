---
acceptance_criteria:
- 'Property test: recording any sequence of transactions through TraceWriter then
  replaying via ReplayLink reproduces identical frames and timings on a VirtualClock'
- ReplayLink fails the test with a precise diff when the code under test sends a request
  that differs from the trace
- The .rrtrace reader is fuzzed with no panics; an unknown trace version is rejected
- Traces carry no VIN or serial unless explicitly marked unscrubbed (field-level marking
  for the scrub command)
created: '2026-10-19'
depends_on:
- US-RR-18
- US-RR-17
epic_id: EPIC-RR-6
id: US-RR-33
points: 5
priority: must
status: ready
tags:
- m2
- comms
- testing
title: Link trait, .rrtrace format and Replay Link
updated: '2026-10-23'
---

As a developer, I want one I/O abstraction for every adapter and a trace format that doubles as a test fixture, so that any real session can be replayed deterministically as a regression test.

- `trait Link { transact(op, req, expect) -> Result<Frame>; stream() -> frames; capabilities() }` (C7).
- `.rrtrace` v1: JSON lines of tx/rx bytes, timestamps (virtual or real), link events (open/close/error/break), quirks applied, op names. Versioned (C16).
- TraceWriter wraps any Link.
- ReplayLink serves a trace on a VirtualClock and fails on any divergence from the recorded requests.

Test tiers: T0, T1, T3, T4. Refs: ARCHITECTURE.md C7/C16, D-018.