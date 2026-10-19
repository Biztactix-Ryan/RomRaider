---
acceptance_criteria:
- With a failing checksum, the default save path does not write; only the explicit
  typed override does (egui_kittest)
- A test enumerates RomWrite token minting sites and fails if any exist outside the
  confirmation flow module
- Overwriting the original source file requires a separate explicit confirmation and
  a .bak is created
- Override saves are visible in the journal viewer with which checksums failed
created: '2026-10-19'
depends_on:
- US-RR-59
- US-RR-45
epic_id: EPIC-RR-8
id: US-RR-62
points: 8
priority: should
status: backlog
tags:
- m4
- ui
- safety
title: Save and override confirmation flows
updated: '2026-10-19'
---

As a tuner, I want saving to clearly show checksum status and require a deliberate, explicit choice to override, so that I can't accidentally produce a ROM that will brick my ECU when flashed.

- Save dialog shows per-checksum validation.
- A failing checksum requires a separate, explicit override confirmation (typed confirmation; no default button).
- This is the only UI that mints RomWrite tokens.
- Overwriting the source file needs its own explicit confirmation.
- The journal records overrides.

Test tiers: T6, T10. Refs: SECURITY.md Authorization, D-013.