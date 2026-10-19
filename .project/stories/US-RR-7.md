---
acceptance_criteria:
- trybuild compile-fail tests prove a token cannot be constructed, cloned into existence,
  defaulted, deserialized or transmuted-free from outside the minting module
- A test enumerates all public functions in rr-rom and rr-comms that perform writes
  (via a marker trait or attribute) and fails if any lacks a token parameter
- 'Tokens are scoped: an EcuMemoryWrite token carries the ECU ID and address range
  it was granted for, and use outside that range is rejected at runtime with a test'
- The T6 suite runs in ci.yml as a release-blocking job
created: '2026-10-19'
depends_on:
- US-RR-1
epic_id: EPIC-RR-1
id: US-RR-7
points: 5
priority: must
status: backlog
tags:
- m0
- safety
- security
title: Capability tokens and safety-invariant suite
updated: '2026-10-19'
---

As a user, I want it to be impossible for the software to write to my ECU or overwrite my ROM without my explicit confirmation, so that a bug can't brick my car.

- Token types RomWrite, EcuMemoryWrite, EcuReset in rr-core, with no public constructor.
- Minting only via a sealed confirmation API that rr-app/rr-cli implement.
- Every write API in rr-rom/rr-comms takes a token by value or reference.
- EcuFlash doesn't exist as a capability.
- T6 suite skeleton that later stories extend.

Test tiers: T6 (compile-fail via trybuild), T7. Refs: SECURITY.md Authorization, D-013.