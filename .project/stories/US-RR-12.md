---
acceptance_criteria:
- Every area listed has a docs/legacy page with legacy/ file:line references for each
  described behaviour
- Each page ends with a fixtures-to-write list, and each listed fixture becomes a
  tracked task or KAT file
- Behaviours that are bugs are cross-referenced to an MF ledger entry rather than
  described as intended
- The SSM page documents init, read-address, read-memory, write and echo handling
  precisely enough to write simulator fixtures from it
created: '2026-10-19'
depends_on: []
epic_id: EPIC-RR-2
id: US-RR-12
points: 8
priority: must
status: backlog
tags:
- m0
- docs
- fixtures
title: Legacy behaviour catalogue (read-only)
updated: '2026-10-19'
---

As a developer, I want each area of the Java RomRaider's behaviour documented as a spec with file references, so that we port intent rather than code, and know which known-answer fixtures to write. Read the source only; never run it.

Areas:
- definition parsing and inheritance
- storage, endian and scaling semantics incl. JEP dialect quirks
- each checksum algorithm
- SSM/K-line protocol flow
- logger definition semantics
- CSV format
- settings.xml schema (for the importer)

Each section ends with a "fixtures to write" list and links to the Must-Fix ledger. Lives in docs/legacy/.

Test tiers: n/a (feeds T2). Refs: D-024, PROJECT.md Must-Fix Bugs.