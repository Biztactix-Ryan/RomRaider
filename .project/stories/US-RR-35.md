---
acceptance_criteria:
- Every non-suspect stock ROM in the external corpus validates with zero failures
  under its family's checksum spec
- compute() on an unmodified stock ROM returns zero patches; after flipping any single
  data byte in a checksummed range, validate fails and compute returns patches touching
  only checksum storage bytes, after which validate passes
- When any segment's validation fails (e.g. E38 segment mismatch), compute returns
  an error and zero patches - never zeroed sums (MF-01, T6)
- Checksums that depend on other checksums are computed in declared order and the
  result is order-independent of spec listing (EC-12); ranges overlapping checksum
  storage are handled (EC-11)
- An unknown named algorithm is an Error diagnostic at load; there is no code path
  that loads algorithms from files (MF-11)
- The Subaru checksum is driven by the ChecksumSpec, not by a table named 'Checksum
  Fix' (MF-19)
- cargo-mutants reports zero surviving mutants in the checksum engine and every registry
  algorithm
created: '2026-10-19'
depends_on:
- US-RR-31
- US-RR-28
- US-RR-11
- US-RR-14
epic_id: EPIC-RR-5
id: US-RR-35
points: 13
priority: must
status: backlog
tags:
- m1
- rom
- checksum
- safety
title: 'Checksum engine: specs + named registry'
updated: '2026-10-19'
---

As a tuner, I want checksums validated and fixed by a spec-driven engine proven against hundreds of real stock ROMs, so that a saved ROM always has correct checksums and a bad algorithm can never zero or corrupt them.

- `ChecksumSpec { algo: Sum{width,endian,init,negate} | Xor | Crc16 | Copy | Named(..), ranges, excludes, store, order }`, layered per ECU family and ID.
- Small compiled-in named registry: Subaru table-based (first, for the target car), Nissan ALT/ALT2 with explicit dependency order, Motronic single/double, E38 with the zeroed-sum bug fixed.
- validate() → per-checksum result. compute() → Patches only, never direct writes.
- Fail-closed: if any segment fails validation, compute returns an error and no patches.
- No runtime class loading.

Test tiers: T0, T1, T2 (standard vectors + stock-ROM invariants), T6, T7. Closes MF-01, MF-11, MF-19; EC-11, EC-12. Refs: D-009.