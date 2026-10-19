---
acceptance_criteria:
- A PR that drops coverage of rr-core/rr-rom/rr-comms below 90% or below the base
  branch fails
- A deliberately weakened test on a gated module produces a surviving mutant and fails
  the mutants job
- Every exclusion in mutants.toml has a justification comment and a CI check rejects
  uncommented exclusions
- The weekly full mutation run publishes its report as a CI artifact
created: '2026-10-19'
depends_on:
- US-RR-2
epic_id: EPIC-RR-1
id: US-RR-4
points: 5
priority: must
status: active
tags:
- m0
- testing
- mutation
- coverage
- safety
title: Coverage and mutation gates
updated: '2026-10-19'
---

As a maintainer, I want coverage and mutation testing enforced on the safety-critical crates, so that tests prove they actually catch broken logic instead of just running it.

- coverage.yml: cargo-llvm-cov; ≥90% line coverage on rr-core, rr-rom, rr-comms; PR coverage may not regress.
- mutants.yml: cargo-mutants --in-diff on PRs touching gated modules, plus a full weekly run.
- Gated modules: rr-core encoding/expr/conversion/layer, rr-rom journal/checksum/table write path, rr-comms engine/write gating.
- Zero survivors; justified exclusions live in mutants.toml with comments.

Test tiers: T7, T8. Refs: PROJECT.md Testing Strategy, D-003.