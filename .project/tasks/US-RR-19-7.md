---
archived: false
assignee: claude
claimed_at: '2026-10-19T13:55:23.287265+00:00'
claimed_by_run: orch-2026-10-19-d2c2
created: '2026-10-19'
depends_on: []
id: US-RR-19-7
points: 3
status: done
story_id: US-RR-19
tags: []
title: Define Encoding types and parse from definition strings
updated: '2026-10-20'
---

rr-core::encoding: Storage (U8..U64, I8..I64, U20, U24, I24, F32, F64), Endian, BitField{mask,shift,signed}, Layout{Default,BoschSubtract}; parse legacy storagetype/endian strings with Error diagnostics for unknown values.