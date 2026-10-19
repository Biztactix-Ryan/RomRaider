---
archived: false
assignee: null
claimed_at: null
claimed_by_run: null
created: '2026-10-19'
depends_on:
- US-RR-6-5
id: US-RR-6-6
points: 3
status: todo
story_id: US-RR-6
tags: []
title: Implement three-tier fixture loader with skip reporting
updated: '2026-10-19'
---

Loader for public (repo fixtures/), external corpus (RR_CORPUS_DIR) and private (RR_FIXTURES_DIR). Missing tier => test reported skipped with the env var to set (nextest-visible), never passed. Rejects unknown schema versions.