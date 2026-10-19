---
archived: false
assignee: null
claimed_at: null
claimed_by_run: null
created: '2026-10-19'
depends_on: []
id: US-RR-17-4
points: 2
status: todo
story_id: US-RR-17
tags: []
title: Implement Diagnostic, Severity and Provenance types
updated: '2026-10-19'
---

rr-core::diag: Diagnostic{severity, code, message, provenance}; Provenance{source, span: LineCol|ByteOffset, layer, rule_id}; Display impls.