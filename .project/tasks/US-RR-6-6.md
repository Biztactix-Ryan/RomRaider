---
archived: false
assignee: claude
claimed_at: '2026-10-19T09:07:23.299224+00:00'
claimed_by_run: orch-2026-10-19-acf8
created: '2026-10-19'
depends_on:
- US-RR-6-5
id: US-RR-6-6
points: 3
status: done
story_id: US-RR-6
tags: []
title: Implement three-tier fixture loader with skip reporting
updated: '2026-10-19'
---

Loader for public (repo fixtures/), external corpus (RR_CORPUS_DIR) and private (RR_FIXTURES_DIR). Missing tier => test reported skipped with the env var to set (nextest-visible), never passed. Rejects unknown schema versions.