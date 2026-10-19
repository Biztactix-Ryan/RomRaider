---
created: '2026-10-19'
id: EPIC-RR-5
points: null
priority: must
status: draft
tags:
- m1
- rom
- safety
target_date: null
title: ROM Core (rr-rom)
updated: '2026-10-19'
---

M1. Byte image + Patch journal as the only source of truth, the N-D Table as views over bytes, table operations written once, the checksum engine, the safe save pipeline, diff/clipboard, and the romraider-cli editor commands. This is the highest brick-risk area; most stories are tagged safety.

Success criteria:
- Every stock ROM in the external corpus verifies, and fix-checksums is a no-op on it.
- Out-of-range edits are rejected, never wrapped.
- Saves are atomic with a .bak and require a RomWrite token.
- Mutation-gated with zero survivors.
- Closes MF-01/03/11/12/14/19/22/39, EC-01/02/05/11/12/13/14.