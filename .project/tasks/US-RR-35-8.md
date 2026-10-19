---
archived: false
assignee: null
claimed_at: null
claimed_by_run: null
created: '2026-10-19'
depends_on: []
id: US-RR-35-8
points: 5
status: todo
story_id: US-RR-35
tags: []
title: Implement ChecksumSpec engine (validate/compute, fail-closed)
updated: '2026-10-19'
---

rr-rom::checksum: validate(image, spec) -> per-checksum results; compute(image, spec) -> Result<Vec<Patch>> that returns an error and zero patches if any segment fails validation; ordered dependencies.