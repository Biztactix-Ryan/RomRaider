---
acceptance_criteria:
- PR template contains a checklist covering test tiers touched, ledger entries closed
  with test names, concept catalogue check and safety-tag review
- CODEOWNERS requires a second approval for changes under the gated safety modules
  and branch protection enforces it
- Issue template instructs users to run romraider-cli trace scrub before attaching
  traces
- CONTRIBUTING.md states the Definition of Done and links PROJECT.md Testing Strategy
  and the ledgers
created: '2026-10-19'
depends_on:
- US-RR-2
epic_id: EPIC-RR-1
id: US-RR-8
points: 2
priority: must
status: backlog
tags:
- m0
- process
title: Contribution rules and Definition of Done
updated: '2026-10-19'
---

As a maintainer, I want the testing-first and concept-reuse rules written down and enforced in the PR flow, so that every contributor (human or agent) follows them.

- CONTRIBUTING.md: test tiers per crate; Must-Fix/Edge-Case ledger rules (an entry closes only with a named passing test); concept catalogue rule (new concept means a DECISIONS entry); fixture source rule; no Java (D-024).
- PR template with a checklist.
- CODEOWNERS requiring a second review on safety modules.
- Issue template asking for scrubbed traces.

Test tiers: n/a (process). Refs: VISION.md Principles, D-002, D-003.