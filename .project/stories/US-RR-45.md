---
acceptance_criteria:
- Saving with a failing checksum and no override token is refused and the destination
  file is untouched (T6)
- A simulated crash at every step of the save (fault injection between write, fsync
  and rename) leaves either the old file or the new file intact, never a partial one,
  and the .bak present (EC-13)
- Save to a read-only directory, a path with spaces, a unicode path and a path on
  a different filesystem than temp all behave correctly with clear errors (EC-14)
- Every saved file re-opens and validates with zero checksum failures (round-trip
  test across the corpus with random edits)
- An override save records who/when/which checksums failed in the sidecar journal
created: '2026-10-19'
depends_on:
- US-RR-35
- US-RR-7
- US-RR-31
epic_id: EPIC-RR-5
id: US-RR-45
points: 8
priority: must
status: backlog
tags:
- m1
- rom
- safety
title: Safe save pipeline
updated: '2026-10-19'
---

As a tuner, I want saving a ROM to be impossible to get wrong, so that I never flash a file with bad checksums or lose my original because of a crash mid-save.

Pipeline:
1. Recompute checksums as patches.
2. Validate all checksums.
3. If any fail, refuse unless an explicit override token is given; record the override in the sidecar journal.
4. Write to a temp file in the same directory, fsync, rename atomically.
5. Keep the previous file as `.bak`.

Saving requires a RomWrite token. Output defaults to a new filename, never silently overwriting the source.

Test tiers: T0, T6, T7. Closes EC-13, EC-14. Refs: SECURITY.md Authorization, ARCHITECTURE.md Safety gates, D-013.