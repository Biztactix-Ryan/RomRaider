---
acceptance_criteria:
- fetch-corpus verifies all 501 files against the committed manifest and fails loudly
  on any hash or size mismatch
- Images with non-power-of-two or non-standard sizes are marked suspect in the manifest
  with a reason, and tests consuming the corpus handle suspect entries explicitly
- No ROM bytes are ever written inside the repository working tree (test asserts RR_CORPUS_DIR
  is outside the repo)
- CI caches the corpus keyed by manifest hash and a cold fetch completes within the
  CI time budget
created: '2026-10-19'
depends_on:
- US-RR-6
epic_id: EPIC-RR-2
id: US-RR-11
points: 5
priority: must
status: backlog
tags:
- m0
- testing
- corpus
title: 'External stock-ROM corpus: fetch, manifest, suspect classification'
updated: '2026-10-19'
---

As a developer, I want the ~500 stock Subaru ROMs available to every test run, pinned and verified, so that checksum and decode logic is tested against real-world images without committing proprietary firmware.

- `cargo xtask fetch-corpus`: fetch from the private Biztactix mirror (TBD), else upstream samuelw9999/RomRaider @639f3c73, into RR_CORPUS_DIR.
- Generate and verify `fixtures/corpus-manifest.toml`: path, size, SHA-256, region, model, ROM ID as read by our own code later.
- Classify suspect images: non-standard sizes such as the 1,048,731- and 1,048,784-byte files.
- Cache the corpus in CI.
- Local mirror already at /home/ryan/Repo/romraider-stock-roms (SHA256SUMS for 501 files).

Test tiers: T0, T2. Refs: D-025, INFRASTRUCTURE.md Hosting & Services.