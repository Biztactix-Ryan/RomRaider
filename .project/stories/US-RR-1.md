---
acceptance_criteria:
- cargo build --workspace and cargo nextest run succeed on a clean checkout with the
  pinned toolchain
- A test reading cargo metadata fails if rr-core depends on any workspace crate or
  on an OS/IO crate (serialport, libloading, nusb, std::fs-using deps)
- A test fails if any crate other than rr-link, rr-cli or rr-app depends on serialport,
  libloading, nusb or winreg
- Binary names are romraider-rs and romraider-cli (not rr) and both print --version
created: '2026-10-19'
depends_on: []
epic_id: EPIC-RR-1
id: US-RR-1
points: 3
priority: must
status: done
tags:
- m0
- foundation
title: Cargo workspace and crate boundaries
updated: '2026-10-19'
---

As a contributor, I want the Cargo workspace scaffolded with the agreed crates and an automated dependency-rule check, so that the architecture's purity boundaries can't silently erode.

Crates: rr-core, rr-defs, rr-rom, rr-link, rr-comms, rr-live, rr-testkit, rr-cli (bin `romraider-cli`), rr-app (bin `romraider-rs`), plus an xtask crate.
Pin the toolchain (rust-toolchain.toml, edition 2024). Commit Cargo.lock. Workspace-level lints table.

Test tiers: T0 (dependency-rule test via cargo metadata).
Refs: ARCHITECTURE.md Crate Map + dependency rule; PROJECT.md Components.