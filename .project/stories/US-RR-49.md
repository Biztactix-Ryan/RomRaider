---
acceptance_criteria:
- x86_64 and i686 MSIs install, run romraider-cli --version and uninstall cleanly
  on a Windows CI runner
- The i686 binary is a genuine 32-bit PE (checked in CI)
- Release notes state signing status explicitly
created: '2026-10-19'
depends_on:
- US-RR-38
epic_id: EPIC-RR-9
id: US-RR-49
points: 5
priority: should
status: backlog
tags:
- release
- windows
title: Windows MSI and i686 build
updated: '2026-10-19'
---

As a Windows user, I want a proper installer, including a 32-bit build for 32-bit-only J2534 DLLs, so that installing on Windows is as easy as on Linux.

- cargo-dist MSI (WiX) for x86_64 and i686, with Start-menu shortcuts and file associations (optional).
- Code signing: TBD (ask: Ryan whether an Authenticode certificate is available). Unsigned until then, clearly labelled.

Test tiers: smoke (install, run --version, uninstall on a Windows runner). Refs: INFRASTRUCTURE.md Deployment Process.