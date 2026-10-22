---
archived: false
assignee: null
claimed_at: null
claimed_by_run: null
created: '2026-10-22'
depends_on: []
id: US-RR-39-9
points: 3
status: todo
story_id: US-RR-39
tags: []
title: Write definition KATs LEG-DEF-19..24 (scalingbase, scaling inheritance, scale
  selection, attributes, user levels, malformed)
updated: '2026-10-22'
---

Write fixtures LEG-DEF-19, LEG-DEF-20, LEG-DEF-21, LEG-DEF-22, LEG-DEF-23, LEG-DEF-24 under `fixtures/defs-kat/romraider/` (or the rr-defs test named). Source: docs/legacy/definitions.md "Fixtures to write", §4.4, §5.1, §6, §8. Hand-derived or transcribed as data from legacy's own test files (D-024).

- LEG-DEF-19 `scalingbase.toml`: the five `<scalingbase>`s of legacy/src/test/definitions/scalingbase_test.xml:4-8 and the expected `Scale` values of TableScaleUnmarshallerTest.java:64-124 (§6.1, §6.3).
- LEG-DEF-20 `scaling-inheritance.toml`: `base` keeps category; name from `name`/`units`/`none`; `increment` alias; units-only override keeps `to_byte` (UL-DEF-03); unknown/forward base (UL-DEF-05); table-local scaling not a base (MF-16) (§6.2).
- LEG-DEF-21 test (rr-defs `select_scaling`): preferred category, else `Default`, regardless of order (UL-DEF-13) (§6.4).
- LEG-DEF-22 `table-attributes.toml`: `dataLayout` inherited (UL-DEF-04), `mask` inherited / empty ignored, `beforeram`, `category` normalisation, `locked`, `logparam`, `description` (§4.4).
- LEG-DEF-23 `user-levels.toml`: `userlevel` absent, 1-5, 7, -1 (MF-37); axis follows parent (UL-DEF-11); the §5.1 editor visibility/edit/save rules as an editor test once the editor exists (US-RR-61).
- LEG-DEF-24 `malformed.toml`: nameless table, `0X` hex, address `0x80000000`, bad `min`, bad size: each an Error at line:col, every other table resolved (MF-23, UL-DEF-14) (§8).

Check existing coverage first (crates/rr-defs/src/romraider/scaling/tests.rs, crates/rr-defs/tests/corpus_romraider_scalings.rs). When written, update docs/legacy/fixtures-map.toml (`kat = [...]`, drop `task`).