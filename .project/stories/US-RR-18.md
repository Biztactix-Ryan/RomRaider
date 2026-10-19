---
acceptance_criteria:
- 'VirtualClock: sleeping 55 ms then 5000 ms advances virtual time exactly and the
  test completes in under 10 ms of real time'
- Two threads sharing a VirtualClock observe a consistent monotonic timeline (property
  test)
- Monotonic time never goes backwards even if wall-clock is changed during a RealClock
  test (wall time is reported separately)
- RealClock is the only module allowed to call std time APIs (enforced by the safety
  lints story)
created: '2026-10-19'
depends_on:
- US-RR-1
epic_id: EPIC-RR-3
id: US-RR-18
points: 3
priority: must
status: done
tags:
- m0
- core
- concepts
- timing
title: 'Clock: RealClock and VirtualClock (C9)'
updated: '2026-10-19'
---

As a developer, I want all time to flow through an injected Clock, so that comms timing, timeouts and quirk delays are deterministic and instant in tests and correct in production.

- `trait Clock { now() -> Instant-like; sleep_until(deadline) }`
- RealClock (the only allowed caller of std time APIs).
- VirtualClock with manual advance and auto-advance-on-sleep modes, shareable across threads for simulator tests.
- Monotonic, with wall-clock exposed separately for log timestamps (EC-26).

Test tiers: T0, T1. Refs: ARCHITECTURE.md C9, D-010, MF-27, EC-26.