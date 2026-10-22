---
archived: false
assignee: null
claimed_at: null
claimed_by_run: null
created: '2026-10-22'
depends_on: []
id: US-RR-39-7
points: 3
status: todo
story_id: US-RR-39
tags: []
title: Write definition KATs LEG-DEF-01, 05, 06, 08, 09, 10 (romid, RAM offset, inheritance,
  omit, table base, addresses)
updated: '2026-10-22'
---

Write fixtures LEG-DEF-01, LEG-DEF-05, LEG-DEF-06, LEG-DEF-08, LEG-DEF-09, LEG-DEF-10 as definition-reader KATs under `fixtures/defs-kat/romraider/` (format: fixtures/README.md, "Definition-reader fixtures"). Source: docs/legacy/definitions.md, "Fixtures to write" (and the sections named per row). Hand-derived from the page (D-024), never from running Java; a pinned legacy bug expects the port's fixed behaviour, the legacy result in `notes`.

- LEG-DEF-01 `romid-elements.toml`: every `<romid>` element of §2.1, any-case element names, untrimmed text; base-inherited fields with the child winning; `obsolete` not inherited; unknown elements warned (§2.1, §3.1).
- LEG-DEF-05 `ram-offset.toml` + synthetic image: `filesize` = image size, image larger/smaller, `noramoffset`, `<offset>`, a `beforeram` table, no `filesize` (EC-02) (§2.3).
- LEG-DEF-06 `inheritance-chain.toml`: three-level chain, single-field overrides, matched ROM wins; two `<rom>`s of one `xmlid` in a file (first wins, warning) (§3.1, §3.2).
- LEG-DEF-08 `omit.toml`: omit mid-list (MF-07), omit-then-override in a grandchild (EC-04), omit of an absent table, omit on an axis (§4.1, §4.2, §4.6).
- LEG-DEF-09 `table-base.toml`: table `base` within a ROM and from an inherited table; copy independence; missing base (UL-DEF-10) (§4.2).
- LEG-DEF-10 `address-inheritance.toml`: axis override without `storageaddress` keeps the base address (UL-DEF-01); axis named like a top-level table (UL-DEF-02); different-case child table name (§4.1, §4.4).

Existing partial coverage to extend rather than duplicate: fixtures/defs-kat/romraider/inheritance.toml. When written, point each id's entry in docs/legacy/fixtures-map.toml at its file (`kat = [...]`) and drop `task` (cargo xtask check-legacy-docs enforces the map).