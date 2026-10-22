---
archived: false
assignee: null
claimed_at: null
claimed_by_run: null
created: '2026-10-22'
depends_on: []
id: US-RR-64-9
points: 2
status: todo
story_id: US-RR-64
tags: []
title: Capture SSM fixtures LEG-SSM-17, 18, 19 on the 2004 Liberty (read-only)
updated: '2026-10-22'
---

Record fixtures LEG-SSM-17, LEG-SSM-18, LEG-SSM-19 as private-tier captures (.rrtrace under RR_FIXTURES_DIR, US-RR-33 format), read-only. Source: docs/legacy/ssm.md "Fixtures to write", §1.5, §3.2, §5, §6.

- LEG-SSM-17: init exchange as received with timestamps: echo, first-reply latency (P2-like), inter-byte gap (P1-like).
- LEG-SSM-18: `A8` single-shot cycles and a continuous run: gap between streamed frames; whether a break, a new command or both stop the stream.
- LEG-SSM-19: the ECU's reaction to an `A8` of an unmapped address and to a request with a bad checksum (silence or reply), deciding the NAK-less timeout policy.

Depends on the .rrtrace format and TraceWriter (US-RR-33). When captured, point each id's entry in docs/legacy/fixtures-map.toml at the committed scrubbed trace or its manifest entry (`kat = [...]`, drop `task`).