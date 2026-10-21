---
acceptance_criteria:
- Billion-laughs, external-entity (XXE) and deeply-nested inputs are rejected with
  diagnostics within bounded time and memory
- ISO-8859-1, UTF-8 with BOM and CRLF variants of the same definition parse to identical
  results
- The XML fuzz target runs nightly with no panics, hangs or unbounded allocation
- Every pinned definition file in the corpus parses without Error diagnostics
created: '2026-10-19'
depends_on:
- US-RR-5
- US-RR-17
epic_id: EPIC-RR-4
id: US-RR-27
points: 5
priority: must
status: done
tags:
- m1
- definitions
- security
title: Hardened XML reading layer
updated: '2026-10-21'
---

As a user loading definition packs from forums, I want XML parsing that can't be abused, so that a malicious or broken definition file can't crash, hang or exploit the app.

- quick-xml-based reader: no DTD processing, no external entities, no entity expansion.
- Limits on depth, attribute size, element count and file size.
- Encoding detection: UTF-8, UTF-8 with BOM, ISO-8859-1. Handles CRLF.
- Errors become Diagnostics with line:col provenance.

Test tiers: T0, T3. Closes EC-09. Refs: SECURITY.md API Security.