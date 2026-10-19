---
acceptance_criteria:
- A CI check fails if any release binary requires a GLIBC symbol newer than 2.28
- romraider-cli --version runs in Ubuntu 20.04 and Rocky 8 containers (x86_64) and
  in an arm64 Debian container under QEMU
- ldd on release binaries shows no libudev dependency and port enumeration works via
  sysfs (test on Linux CI)
created: '2026-10-19'
depends_on:
- US-RR-38
epic_id: EPIC-RR-9
id: US-RR-42
points: 5
priority: should
status: backlog
tags:
- release
- linux
- aarch64
title: glibc 2.28 baseline and aarch64 builds
updated: '2026-10-19'
---

As a Linux user on an older distro or a Raspberry Pi, I want one binary that runs everywhere, so that I don't need to compile anything.

- cargo-zigbuild targeting glibc 2.28 for x86_64 and aarch64.
- aarch64 tarball, AppImage and .deb (arm64).
- A CI check that the binary's max required GLIBC symbol version is ≤ 2.28.
- serialport built without libudev, with port enumeration via sysfs.

Test tiers: smoke. Refs: INFRASTRUCTURE.md Linux Release, D-022.