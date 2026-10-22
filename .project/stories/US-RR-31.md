---
acceptance_criteria:
- 'Property test: for any sequence of patches, undo-all restores the original bytes
  exactly and redo-all reproduces the final bytes exactly'
- A patch whose old bytes do not match the current image is rejected (stale-patch
  protection)
- The sidecar journal round-trips losslessly (property test) and rejects a journal
  whose base image hash does not match
- There is no public API that mutates image bytes without producing a patch - enforced
  by module visibility and a compile-fail test
- cargo-mutants reports zero surviving mutants in the journal module
created: '2026-10-19'
depends_on:
- US-RR-6
- US-RR-17
epic_id: EPIC-RR-5
id: US-RR-31
points: 5
priority: must
status: done
tags:
- m1
- rom
- safety
title: Byte image and Patch journal (C10)
updated: '2026-10-22'
---

As a tuner, I want every change to my ROM recorded as an exact byte patch with full undo/redo, so that nothing is lost, I can see exactly what changed, and overlapping tables can never desync.

- `Image` (owned bytes plus original snapshot).
- `Patch { range, old, new, origin: Edit|Paste|ChecksumFix|ApplyDiff }`.
- Journal with undo/redo, grouping (one user action = one undo step), and modified-byte queries.
- Sidecar `.rrjournal` serialisation including checksum overrides.
- No mutation API except through patches.

Test tiers: T0, T1, T7. Refs: ARCHITECTURE.md C10, D-006.