---
created: '2026-10-19'
id: EPIC-RR-9
points: null
priority: must
status: draft
tags:
- release
- linux
- packaging
target_date: null
title: Release & Linux Packaging
updated: '2026-10-19'
---

M0 → M6. Linux is first-class (D-022). The release skeleton from M0 (nightly romraider-cli tarball, AppImage and .deb), then:
- glibc 2.28 baseline and aarch64 builds
- .rpm, AUR and install.sh
- package smoke tests across 7 distros
- udev rules generated from adapter profiles, plus `doctor` (EACCES, ModemManager, brltty, FTDI latency)
- Windows MSI and i686 builds

Success criteria:
- Every release ships all artifacts with checksums.
- Package smoke tests gate releases.
- KKL cable works on a fresh Ubuntu install with no manual permission steps.