---
archived: false
assignee: null
claimed_at: null
claimed_by_run: null
created: '2026-10-19'
depends_on: []
id: US-RR-4-6
points: 3
status: todo
story_id: US-RR-4
tags: []
title: Add mutants.yml and mutants.toml
updated: '2026-10-19'
---

cargo-mutants --in-diff on PRs touching gated module paths; weekly full run on gated modules; zero-survivor gate; report artifact. mutants.toml lists gated paths and exclusions.