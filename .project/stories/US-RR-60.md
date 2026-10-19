---
acceptance_criteria:
- log and identify against the simulator produce CSV and ID output matching the simulator's
  configured ECU (integration test)
- replay of a captured simulator trace reproduces the same CSV byte-for-byte
- scrub removes every VIN and serial field from a trace and log (planted-value test)
  and the result still replays
- No CLI command can issue an ECU write; log, capture and identify are read-only by
  construction (T6)
created: '2026-10-19'
depends_on:
- US-RR-47
- US-RR-37
- US-RR-54
- US-RR-57
epic_id: EPIC-RR-6
id: US-RR-60
points: 5
priority: must
status: backlog
tags:
- m2
- cli
- comms
title: romraider-cli log, capture, replay and scrub
updated: '2026-10-19'
---

As the owner of the Liberty, I want to log my car from the command line and capture raw traces, so that I can start collecting real data and regression fixtures before any GUI exists.

Commands:
- `log --port <by-id> --params ... -o session.csv` (read-only; writes CSV through the SampleFrame pipeline)
- `capture` (raw .rrtrace of everything, including identify)
- `replay <trace>` (through the engine, with output CSV)
- `trace scrub` / `log scrub` (redact VIN, serials)
- `identify` (ECU ID, ROM ID, supported params)

Note: this pulls the SampleFrame pipeline and the CSV writer (E7) forward into M2.

Test tiers: T0, T4, T5. Refs: SECURITY.md PII Handling, D-018.