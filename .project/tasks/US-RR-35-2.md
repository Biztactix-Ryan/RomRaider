---
archived: false
assignee: null
claimed_at: null
claimed_by_run: null
created: '2026-10-19'
depends_on: []
id: US-RR-35-2
points: null
status: todo
story_id: US-RR-35
tags: []
title: 'Test: compute() on an unmodified stock ROM returns zero patches; after flipping
  any single data byte in a checksummed...'
updated: '2026-10-19'
---

Verify acceptance criterion for story US-RR-35:

> compute() on an unmodified stock ROM returns zero patches; after flipping any single data byte in a checksummed range, validate fails and compute returns patches touching only checksum storage bytes, after which validate passes