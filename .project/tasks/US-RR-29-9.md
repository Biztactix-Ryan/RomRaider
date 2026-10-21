---
archived: false
assignee: claude
claimed_at: null
claimed_by_run: null
created: '2026-10-19'
depends_on:
- US-RR-27-5
id: US-RR-29-9
points: 1
status: done
story_id: US-RR-29
tags: []
title: Add ECU definition reader fuzz target
updated: '2026-10-21'
---

Add a cargo-fuzz target that feeds arbitrary bytes through rr-defs::xml and the rr-defs::romraider reader + DefinitionSet resolve (single-file set), asserting no panics, bounded time/allocation under the fuzz limits, and diagnostic/span invariants. Seeds: small hand-written ECU definitions covering every table kind, scaling, base/omit. Wire into fuzz/Cargo.toml, fuzz/src/lib.rs TARGETS, docs/fuzzing.md so the nightly picks it up.