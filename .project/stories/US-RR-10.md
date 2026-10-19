---
acceptance_criteria:
- The dependency-rule test fails if the reference codec module imports anything from
  rr-core encoding or conversion
- The reference codec passes the seed known-answer fixtures for every storage type
  independently of rr-core
- gen-synthetic output is byte-identical across runs and platforms (CI compares hashes
  on Linux and Windows)
- Generated ROMs exercise overlapping tables, tables ending exactly at ROM end, NaN/Inf/denormal
  floats and min/max values for every width
created: '2026-10-19'
depends_on:
- US-RR-6
epic_id: EPIC-RR-2
id: US-RR-10
points: 8
priority: must
status: backlog
tags:
- m0
- testing
- fixtures
- safety
title: Independent reference encoder/decoder and synthetic ROM generator
updated: '2026-10-19'
---

As a developer, I want a deliberately naive second implementation of byte encoding and decoding that shares no code with rr-core, plus a generator that builds synthetic ROMs from known values, so that public CI has real ROM-shaped test data and an independent cross-check of Encoding.

The reference codec works byte by byte with explicit shifts, covering every storage type, endianness, bitfields, MOVI20 and the Bosch layout. The generator takes a definition plus a value plan (incl. extremes, NaN/Inf, overlaps, boundary addresses) and emits the ROM and a manifest of expected values. `cargo xtask gen-synthetic` regenerates byte-identical output.

Test tiers: T1, T2. Refs: PROJECT.md T2(b), D-024, EC-01/06/07.