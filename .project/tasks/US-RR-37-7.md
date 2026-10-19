---
archived: false
assignee: null
claimed_at: null
claimed_by_run: null
created: '2026-10-19'
depends_on:
- US-RR-37-6
id: US-RR-37-7
points: 2
status: todo
story_id: US-RR-37
tags: []
title: Implement K-line echo strip and verify
updated: '2026-10-19'
---

Compare echoed bytes to transmitted; mismatch => LinkError::Bus; only post-echo bytes reach the framer.