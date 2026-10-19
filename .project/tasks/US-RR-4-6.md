---
archived: false
assignee: claude
claimed_at: '2026-10-19T12:12:50.902507+00:00'
claimed_by_run: orch-2026-10-19-d2c2
created: '2026-10-19'
depends_on: []
id: US-RR-4-6
points: 3
status: done
story_id: US-RR-4
tags: []
title: Add mutants.yml and mutants.toml
updated: '2026-10-19'
---

cargo-mutants --in-diff on PRs touching gated module paths; weekly full run on gated modules; zero-survivor gate; report artifact. mutants.toml lists gated paths and exclusions.