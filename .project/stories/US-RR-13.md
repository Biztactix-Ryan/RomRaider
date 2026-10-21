---
acceptance_criteria:
- Attempting to commit a stock ROM from the corpus under fixtures/public is blocked
  by the pre-commit hook and by CI
- A trace containing a 17-character VIN pattern under fixtures/public fails CI
- Private-fixture tests run when RR_FIXTURES_DIR is set and report skipped otherwise
- The guard itself has tests with planted positive and negative samples
created: '2026-10-19'
depends_on:
- US-RR-2
epic_id: EPIC-RR-2
id: US-RR-13
points: 3
priority: must
status: done
tags:
- m0
- security
- fixtures
title: Private corpus wiring and ROM-leak guard
updated: '2026-10-21'
---

As a maintainer, I want our own ROMs and car traces usable in tests but impossible to commit publicly, so that we never leak proprietary firmware or VINs.


- RR_FIXTURES_DIR wiring and the `private-fixtures` feature.

- A pre-commit hook plus a CI check rejecting binaries over 64 KB, files whose bytes match any manifest SHA-256 or known ROM-ID pattern, and unscrubbed VIN patterns in traces under fixtures/public.

- Location of the private corpus: `~/RomRaider-private` (outside the repo), backed up with Ryan's normal home-directory backup. Decision (Ryan, 2026-10-20). Tests skip (and report skipped) when RR_FIXTURES_DIR is unset.


Test tiers: T0. Refs: SECURITY.md Proprietary firmware + PII Handling, D-017.