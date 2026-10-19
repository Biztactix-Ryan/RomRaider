---
archived: false
assignee: claude
claimed_at: '2026-10-19T11:07:32.707821+00:00'
claimed_by_run: orch-2026-10-19-d2c2
created: '2026-10-19'
depends_on: []
id: US-RR-17-4
points: 2
status: done
story_id: US-RR-17
tags: []
title: Implement Diagnostic, Severity and Provenance types
updated: '2026-10-19'
---

rr-core::diag: Diagnostic{severity, code, message, provenance}; Provenance{source, span: LineCol|ByteOffset, layer, rule_id}; Display impls.