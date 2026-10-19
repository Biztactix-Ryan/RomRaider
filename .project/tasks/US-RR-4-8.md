---
archived: false
assignee: null
claimed_at: null
claimed_by_run: null
created: '2026-10-20'
depends_on: []
id: US-RR-4-8
points: 2
status: todo
story_id: US-RR-4
tags:
- ci
- safety
title: Always-run gate jobs so Mutants/Fixtures can be required status checks
updated: '2026-10-20'
---

Maintainer decision 2026-10-20: make coverage-gate, the Mutants PR job and Fixtures validation required status checks on rust-rewrite (not master).

Path-filtered workflows never start on unrelated PRs, so a required check would sit at "Expected" forever. Fix:
- mutants.yml / fixtures.yml: drop trigger-level `paths:` on pull_request (and push for fixtures); add a first job that computes whether relevant paths changed (git diff vs base; no third-party action unless already used), and make the real job(s) skip cleanly via `if:` while a stable-named aggregate job (e.g. `mutants-ok`, `fixtures-ok`) always reports success/failure correctly (failure or cancellation of a needed job => failure; skipped because out of scope => success).
- Keep the schedule/workflow_dispatch behaviour unchanged.
- Update xtask/tests/*_ci_config.rs so the path list stays in step with .cargo/mutants.toml and the aggregate job semantics are enforced; sabotage-prove.
- Document the required check names in CONTRIBUTING.md / INFRASTRUCTURE.md.
Orchestrator applies branch protection (required contexts) after the change is pushed and green.