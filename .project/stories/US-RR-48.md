---
acceptance_criteria:
- All 7 distro containers pass install, version, doctor, fixture decode and uninstall
  in CI
- A deliberately broken package (missing binary) fails the release workflow before
  publishing
- Uninstall leaves no files outside user data directories
created: '2026-10-19'
depends_on:
- US-RR-43
- US-RR-42
epic_id: EPIC-RR-9
id: US-RR-48
points: 5
priority: should
status: backlog
tags:
- release
- linux
- testing
title: Package smoke tests across distros
updated: '2026-10-19'
---

As a maintainer, I want every package installed and exercised on real distros before release, so that a packaging regression never reaches users.

Matrix: Ubuntu 20.04/22.04/24.04, Debian 12, Fedora latest, Rocky 8, Arch. Each runs:
- install the native package
- `romraider-cli --version`
- `romraider-cli doctor --no-devices`
- decode of a synthetic fixture ROM
- uninstall

The AppImage is tested on Ubuntu 20.04 and Fedora. Gates the release workflow.

Test tiers: smoke. Refs: INFRASTRUCTURE.md Package smoke tests.