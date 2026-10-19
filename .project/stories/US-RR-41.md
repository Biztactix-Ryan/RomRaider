---
acceptance_criteria:
- On a VirtualClock the engine never sends a request earlier than P3min after the
  last response, and applies each quirk delay exactly - asserted from trace timestamps
  (EC-23)
- A corrupted or truncated response yields None values and a link-health event; a
  test proves the previous good response is never re-emitted (MF-08)
- Timeouts are wall-clock deadlines from Clock, independent of how often the link
  is polled (MF-27)
- ECU going silent mid-session triggers the recovery policy then a clean disconnect
  with all values None (EC-15); missing or delayed echo is handled per profile (EC-16)
- An unknown ECU ID runs with base-profile layers only and emits a Warning (EC-20)
- cargo-mutants reports zero surviving mutants in timing enforcement and response
  validation
created: '2026-10-19'
depends_on:
- US-RR-33
- US-RR-25
- US-RR-26
- US-RR-28
- US-RR-36
epic_id: EPIC-RR-6
id: US-RR-41
points: 13
priority: must
status: backlog
tags:
- m2
- comms
- safety
title: 'Transaction engine: timing, framing, echo, recovery, quirks'
updated: '2026-10-19'
---

As a user logging my car, I want one engine to handle all protocol timing, framing and error recovery, so that live data is correct, gaps are visible, and a flaky connection never shows stale numbers as live.

- Engine over any Link: enforces P1–P4 from the resolved LinkProfile using Clock deadlines (no polling loops).
- Applies flattened quirk Rules per op; strips and verifies echo; frames responses via Framing.
- Runs the recovery policy (clear line, re-init, give up) on errors.
- Re-resolves profile layers after ECU identification.
- A bad or missing response gives `None` for affected values plus a link-health event, never the previous good response.
- Every transaction and applied quirk goes to the Trace.

Test tiers: T0, T1, T4, T5, T7. Closes MF-08, MF-27; EC-15, EC-16, EC-20, EC-23. Refs: D-004, D-010, D-012.