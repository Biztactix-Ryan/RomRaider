---
acceptance_criteria:
- A spike report documents which address ranges respond to A0, the working block size
  and measured throughput, with traces
- 'If feasible: two full reads produce identical SHA-256 and the image validates under
  its checksum spec, and it is stored in the private corpus only'
- The spike code issues no write opcodes (verified by its trace)
- Outcome and next step recorded as a DECISIONS entry
created: '2026-10-19'
depends_on:
- US-RR-60
epic_id: EPIC-RR-6
id: US-RR-65
points: 5
priority: should
status: backlog
tags:
- m2
- spike
- ssm
- hil
title: 'Spike: read the ROM over SSM read-memory (read-only)'
updated: '2026-10-19'
---

As the owner of the Liberty, I want to know whether my ECU's ROM can be read over K-line with the KKL cable alone, so that we can get my exact stock ROM as ground truth without buying a Tactrix cable.

Spike questions:
- Does the ECU answer SSM read-memory (A0) across the ROM address range, or only RAM?
- What block size works?
- How long would a full read take at 4800 baud?
- Is the result consistent across two reads (hash)?

Strictly read-only, time-boxed. If it works, read the full ROM, validate checksums against the corpus definition, and add it to the private corpus. If not, document that reading needs EcuFlash + a Tactrix cable once.

Test tiers: T9 (spike report). Refs: E6 epic, D-013.