---
acceptance_criteria:
- A mock PassThru shared library built in-tree exercises connect, ioctl SET_CONFIG,
  read/write msgs and error paths in CI
- A J2534 library placed in the current working directory is never loaded (test)
- Loading a 32-bit DLL from a 64-bit process produces a specific diagnostic recommending
  the i686 build
- No unsafe block outside the FFI module, and every unsafe block has a SAFETY comment
  (lint)
created: '2026-10-19'
depends_on:
- US-RR-41
epic_id: EPIC-RR-10
id: US-RR-69
points: 8
priority: should
status: backlog
tags:
- m6
- comms
- j2534
- security
title: J2534 Link
updated: '2026-10-19'
---

As a user with a J2534 adapter (Tactrix OpenPort etc.), I want it supported as a Link, so that newer cars and faster adapters work.

- libloading PassThru API wrapper (v04.04).
- Windows discovery via the PassThruSupport.04.04 registry only; Linux/macOS via a fixed system path list plus RR_J2534_LIB. Never CWD or definition folders; loaded path logged.
- Timing pushed via SET_CONFIG from the LinkProfile.
- 32-bit-only DLL on a 64-bit build gives a clear diagnostic with i686 guidance.
- All FFI wrapped in safe types and fuzz-tested at the boundary.

Test tiers: T0, T3, T5 (mock PassThru library), T9. Closes EC-19. Refs: SECURITY.md Native libraries, D-023.