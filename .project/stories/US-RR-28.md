---
acceptance_criteria:
- 'Every schema round-trips: load(save(x)) == x (property test)'
- A profile with an older schema version migrates to the current one with a snapshot
  test per migration step; an unknown future version is rejected with a diagnostic
- Unknown keys are errors with line:col provenance (no silent ignore)
- TOML profile parsing is fuzzed with no panics
created: '2026-10-19'
depends_on:
- US-RR-22
- US-RR-25
- US-RR-26
epic_id: EPIC-RR-4
id: US-RR-28
points: 5
priority: must
status: backlog
tags:
- m1
- definitions
- profiles
title: TOML profile schemas with versioning
updated: '2026-10-19'
---

As a definition author, I want typed, versioned TOML schemas for link profiles, quirk rules, device profiles and checksum specs, so that supporting a new ECU, cable or device is a reviewed data file with migrations, not code.

Schemas:
- LinkProfile (baud, Timing P1–P4, Framing, echo, init, recovery)
- Rule sets
- DeviceProfile (link settings, framing, fields)
- ChecksumSpec

All loaded through Layered (C1). Each has `schema = N` and migration functions from older schemas. A JSON Schema is exported for editor tooling.

Test tiers: T0, T1, T3. Refs: ARCHITECTURE.md Contracts, D-004, D-014.