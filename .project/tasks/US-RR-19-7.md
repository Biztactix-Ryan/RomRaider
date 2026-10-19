---
archived: false
assignee: null
claimed_at: null
claimed_by_run: null
created: '2026-10-19'
depends_on: []
id: US-RR-19-7
points: 3
status: todo
story_id: US-RR-19
tags: []
title: Define Encoding types and parse from definition strings
updated: '2026-10-19'
---

rr-core::encoding: Storage (U8..U64, I8..I64, U20, U24, I24, F32, F64), Endian, BitField{mask,shift,signed}, Layout{Default,BoschSubtract}; parse legacy storagetype/endian strings with Error diagnostics for unknown values.