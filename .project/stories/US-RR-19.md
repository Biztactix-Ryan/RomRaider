---
acceptance_criteria:
- 'Property test: decode(encode(x)) == x for every storage/endian/bitfield/layout
  combination over the full value range, and the result agrees with the independent
  reference codec'
- U24, I24 and I64 values decode correctly; an unsupported storage string in a definition
  produces an Error diagnostic, never a zero value (MF-02)
- Two images with different endianness decode correctly in the same process concurrently
  (MF-04)
- Float min/max ranges are derived from the Encoding and are correct for F32/F64 signed
  and unsigned contexts (MF-15)
- NaN, +/-Inf and denormal floats decode faithfully and are flagged so edit layers
  can refuse them (EC-07)
- cargo-mutants reports zero surviving mutants in the encoding module
created: '2026-10-19'
depends_on:
- US-RR-17
- US-RR-6
epic_id: EPIC-RR-3
id: US-RR-19
points: 8
priority: must
status: backlog
tags:
- m0
- core
- concepts
- safety
title: 'Encoding (C3): one codec for every storage type'
updated: '2026-10-19'
---

As a tuner, I want every byte-to-raw-value conversion done by one exhaustively tested codec, so that no table, logger parameter or device field is ever decoded or written with the wrong width, sign or byte order.

`Encoding { storage, endian, bitfield: Option<mask/shift>, layout: Default | BoschSubtract }`:
- storage: U8–U64, I8–I64, U20 (MOVI20), U24/I24, F32/F64
- `decode(&[u8]) -> Result<Raw>` / `encode(Raw) -> Result<bytes>`
- No static or global state.
- Unknown or unsupported combinations are load errors, never 0.

Shared by ROM cells, logger parameters, device fields and protocol headers.

Test tiers: T0, T1, T2 (seed KATs + reference codec), T7. Closes MF-02, MF-04, MF-15; EC-06, EC-07. Refs: ARCHITECTURE.md C3.