---
acceptance_criteria:
- verify exits 0 on every non-suspect corpus ROM and non-zero with a per-checksum
  report on a corrupted copy
- edit without the explicit write confirmation flag refuses to write; edit never overwrites
  the input path unless --in-place and confirmation are both given
- decode output for synthetic ROMs matches the generator manifest exactly (integration
  test)
- All commands return structured JSON errors with diagnostic codes when --json is
  set (snapshot tests)
created: '2026-10-19'
depends_on:
- US-RR-45
- US-RR-46
- US-RR-44
epic_id: EPIC-RR-5
id: US-RR-50
points: 5
priority: must
status: backlog
tags:
- m1
- cli
- rom
title: romraider-cli editor commands
updated: '2026-10-19'
---

As a tuner (and as the test suite), I want headless commands for every ROM operation, so that the whole ROM core is usable and tested end to end without a GUI.

Commands:
- `decode <rom> [--table]` (JSON/CSV output)
- `verify <rom>` (checksums, exit code)
- `diff <a> <b>`
- `edit <rom> --table T --set/--inc/--mul ... -o out` (needs explicit --write confirmation to mint RomWrite; never in-place by default)
- `fix-checksums <rom> -o out`
- `explain <rom> --table T` (definition provenance)

Integration tests run every command against synthetic ROMs and the external corpus.

Test tiers: T0, T2, T6 (integration). Refs: ARCHITECTURE.md rr-cli.