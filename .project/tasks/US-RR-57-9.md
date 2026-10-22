---
archived: false
assignee: null
claimed_at: null
claimed_by_run: null
created: '2026-10-22'
depends_on: []
id: US-RR-57-9
points: 3
status: todo
story_id: US-RR-57
tags: []
title: Write legacy CSV importer goldens LEG-CSV-01..06
updated: '2026-10-22'
---

Write fixtures LEG-CSV-01, LEG-CSV-02, LEG-CSV-03, LEG-CSV-04, LEG-CSV-05, LEG-CSV-06: CSV golden files under `fixtures/public/csv/` (hand-written, small, no VIN: leak-guard rule 5) with importer tests in rr-live. Source: docs/legacy/csv.md "Fixtures to write", §3-7, §9, §13. Hand-computed (D-024).

- LEG-CSV-01 `legacy-relative-comma.csv`: the §13 `en_US` file, `LF` and `CR LF`: header into name and units, `Time (msec)`, three rows.
- LEG-CSV-02 `legacy-relative-semicolon.csv`: the §13 `de_DE` file: `;` delimiter, `,` decimals.
- LEG-CSV-03 `legacy-absolute-time.csv`: `Time` header with `HH:mm:ss.SSS`, midnight wrap -> monotonic timeline (§5).
- LEG-CSV-04 `legacy-header-rewrite.csv`: a second header mid-file (column added, unit changed): three segments; playback equals source (MF-30, EC-27) (§9).
- LEG-CSV-05 `legacy-encoding-cp1252.csv`, `legacy-encoding-utf8.csv`: `°F`, `µs`, `m/s²`, a German name: same text (UL-CSV-02) (§3).
- LEG-CSV-06 importer test: `-0`/`-0.00` read as zero; HALF_EVEN text taken as written; `,`-delimited with `,` decimals refused with a Diagnostic (§6.3, §7).

Serves US-RR-57-4. When written, point each id's entry in docs/legacy/fixtures-map.toml at its file (`kat = [...]`, drop `task`).