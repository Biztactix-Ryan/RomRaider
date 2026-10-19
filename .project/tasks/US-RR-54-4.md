---
archived: false
assignee: null
claimed_at: null
claimed_by_run: null
created: '2026-10-19'
depends_on: []
id: US-RR-54-4
points: 2
status: todo
story_id: US-RR-54
tags: []
title: Define Signal and SampleFrame types with dense index
updated: '2026-10-19'
---

rr-live: Signal{id, unit, role}; SampleFrame{t_mono, t_wall, values: Vec<Option<f64>>}; SignalIndex mapping ids to dense slots.