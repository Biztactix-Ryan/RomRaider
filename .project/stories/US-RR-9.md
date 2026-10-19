---
acceptance_criteria:
- The validator rejects any expected value without a source field, and CI runs it
  on every fixture
- A fixture with hand-calc source must include the working (formula and intermediate
  values) or it is rejected
- At least 10 seed fixtures exist covering every storage type the format supports,
  each reviewed and sourced
- Changing an expected value in an existing fixture requires a changelog line in the
  fixture explaining why (CI check)
created: '2026-10-19'
depends_on:
- US-RR-6
epic_id: EPIC-RR-2
id: US-RR-9
points: 5
priority: must
status: backlog
tags:
- m0
- testing
- fixtures
title: Known-answer fixture format
updated: '2026-10-19'
---

As a developer, I want a reviewed, human-readable known-answer fixture format, so that expected values are hand-checked, sourced and not derived from the code under test (or from Java).

TOML schema (`schema = 1`): a definition snippet or reference, input bytes (hex) or a ROM reference, and expected raw and real values or checksum before/after. Every expected value has a `source`: hand-calc with working shown, standard document + section, stock-ROM invariant, car trace, or cross-source.

Includes the loader in rr-testkit, a schema validator, a fixtures.yml CI job, and a README on authoring fixtures.

Test tiers: T0, T2. Refs: PROJECT.md T2, ARCHITECTURE.md Contracts, D-024.