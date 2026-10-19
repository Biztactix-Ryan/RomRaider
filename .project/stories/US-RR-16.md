---
acceptance_criteria:
- The definition source, its licence and the pinned version are recorded in DECISIONS.md
  and the manifest
- fetch-corpus fetches and hash-verifies the definitions alongside the ROMs, and a
  local mirror exists
- A coverage report lists every corpus ROM ID with its matching definition or 'no
  definition', committed as a fixture and checked in CI
created: '2026-10-19'
depends_on:
- US-RR-11
epic_id: EPIC-RR-2
id: US-RR-16
points: 3
priority: must
status: active
tags:
- m0
- corpus
- definitions
title: 'ECU-definition corpus: source, pin, manifest'
updated: '2026-10-20'
---

As a developer, I want the RomRaider ECU definitions that match the stock-ROM corpus sourced, pinned and verified like the ROMs, so that the corpus suite can decode every table of every stock ROM.

- Identify the authoritative maintained source of RomRaider ECU definitions for Subaru (forum or GitHub; verify, don't assume).
- Pin it to a commit or version and mirror it locally.
- Add it to the manifest, and extend fetch-corpus.
- Report coverage: which corpus ROM IDs have definitions, and which don't.
- Specifically check whether the 2004 AU Liberty NA ROM IDs are covered.

Test tiers: T2. Refs: D-025, INFRASTRUCTURE.md.