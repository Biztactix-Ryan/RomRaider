---
archived: false
assignee: null
claimed_at: null
claimed_by_run: null
created: '2026-10-22'
depends_on: []
id: US-RR-39-8
points: 3
status: todo
story_id: US-RR-39
tags: []
title: Write definition KATs LEG-DEF-11..18, LEG-ENC-04, LEG-ENC-19 (storage, endian,
  table types, axes, 3D layout, switch, bits, float endian, static data)
updated: '2026-10-22'
---

Write fixtures LEG-DEF-11, LEG-DEF-12, LEG-DEF-13, LEG-DEF-14, LEG-DEF-15, LEG-DEF-16, LEG-DEF-17, LEG-DEF-18, LEG-ENC-04, LEG-ENC-19 as definition-reader KATs under `fixtures/defs-kat/romraider/` (or the test named). Source: docs/legacy/definitions.md and docs/legacy/encoding-scaling.md, "Fixtures to write". Hand-derived (D-024).

- LEG-DEF-11 `storage-types.toml`: every `storagetype` spelling of definitions.md §4.4, child flipping `uint8`→`int8` (MF-06), no storagetype (Error, MF-02), `UINT8` (UL-DEF-17).
- LEG-DEF-12 `endian.toml`: `big`/`little`/`1`/`2`/garbage; `<memmodel endian>` override and its inheritance edge cases (UL-DEF-06, UL-DEF-17) (§4.4, §5.3).
- LEG-DEF-13 test: a big- and a little-endian ROM resolved together decode independently (MF-04) (§5.3).
- LEG-DEF-14 `table-types.toml`: each `type` of §4.3, `X Axis` top-level, missing/unknown type, child changing `type` (ignored).
- LEG-DEF-15 `axes.toml`: 2D X/Y axis child, 3D axes, axis count forced, nested non-axis ignored, static axis by `<data>`, size mismatch (UL-DEF-12), extra `<data>` (§4.5, §4.6).
- LEG-DEF-16 `table-3d-layout.toml`: `swapxy`, `flipx`, `flipy`, `skipCells`; `sizey` overridden in a child (legacy scalingbase_test.xml:82) (§4.4).
- LEG-DEF-17 `switch-states.toml`: comma/space data, 2-byte LE swap, state replaced by name in a child, cell count from `sizey`; `Checksum Fix` flagged not special-cased (MF-19) (§4.7).
- LEG-DEF-18 `bitwise-bits.toml`: decimal positions, bits re-listed in a child (UL-DEF-08), position >= width (Error), `<bit>` on a non-bitwise table (warning) (§4.8, §9).
- LEG-ENC-04 `float-endian.toml`: float table `endian="little"` with/without `<memmodel endian>`; two ROMs decode independently (MF-04) (encoding-scaling.md §2.2, §2.3).
- LEG-ENC-19 `static-data.toml`: static `<data>` cells are raw values through `expression`; non-numeric text kept (encoding-scaling.md §4.3).

Existing partial coverage: fixtures/defs-kat/romraider/table-3d.toml, table-switch.toml, table-bitwise-switch.toml, table-2d.toml. When written, update docs/legacy/fixtures-map.toml (`kat = [...]`, drop `task`).