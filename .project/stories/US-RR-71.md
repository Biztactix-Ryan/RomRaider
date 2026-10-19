---
acceptance_criteria:
- The AT initialisation sequence is generated from the profile with no hard-coded
  command strings in Link code
- An ELM simulator covers OK/ERROR/NO DATA/BUS INIT failure responses and clone quirk
  rules
- Prompt framing handles partial and multi-line responses (property test)
created: '2026-10-19'
depends_on:
- US-RR-33
- US-RR-41
epic_id: EPIC-RR-10
id: US-RR-71
points: 5
priority: could
status: backlog
tags:
- m6
- comms
- elm327
title: ELM327 Link
updated: '2026-10-19'
---

As a user with a cheap ELM327 adapter, I want it supported, including clone quirks, so that common adapters work reliably.

- AT command setup expressed as LinkProfile init plus adapter Rules: echo off, headers, protocol select, custom baud.
- Clone quirks as adapter-layer rules with notes.
- Prompt-based framing (Line with `>`).
- Simulator for ELM AT behaviour.

Test tiers: T0, T1, T3, T5, T9.