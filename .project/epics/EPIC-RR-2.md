---
created: '2026-10-19'
id: EPIC-RR-2
points: null
priority: must
status: draft
tags:
- m0
- testing
- fixtures
target_date: null
title: Test Oracles & Fixture Corpus (no Java)
updated: '2026-10-19'
---

M0. Java-free ground truth (D-024, D-025): known-answer fixtures with stated sources, an independent reference encoder/decoder and synthetic ROM generator, the external stock-ROM corpus (samuelw9999/RomRaider @639f3c73, mirrored locally) with a pinned manifest, standard test vectors, a cross-source agreement harness, the ECU-definition corpus, a legacy behaviour catalogue (from reading the Java, never running it), and the private corpus with a ROM-leak guard.

Success criteria:
- Every expected value in a fixture has a stated source.
- The corpus fetch verifies all 501 images against the manifest; suspect images are classified, not crashed on.
- The synthetic generator shares no code with rr-core.
- No ROM can be committed under fixtures/public.