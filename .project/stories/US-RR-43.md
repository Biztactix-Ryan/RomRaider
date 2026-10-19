---
acceptance_criteria:
- The .rpm installs and romraider-cli --version runs in clean Fedora and Rocky 8 containers
- The PKGBUILD builds and installs in a clean Arch container (makepkg) and namcap
  reports no errors
- install.sh user-local install and uninstall leave no files behind (test diffs the
  filesystem before and after); install.sh never requires root unless --system
created: '2026-10-19'
depends_on:
- US-RR-38
epic_id: EPIC-RR-9
id: US-RR-43
points: 5
priority: should
status: backlog
tags:
- release
- linux
- packaging
title: .rpm, AUR PKGBUILD and install.sh
updated: '2026-10-19'
---

As a Fedora, RHEL, Arch or other-distro user, I want a native package or a simple installer, so that installing is one command.

- cargo-generate-rpm (x86_64, aarch64).
- packaging/aur/romraider-rs-bin PKGBUILD, auto-bumped by the release workflow.
- tarball install.sh: user-local into ~/.local by default, `--system` for /usr/local; installs desktop file, icon and udev rules (system mode only); provides uninstall.

Test tiers: smoke. Refs: INFRASTRUCTURE.md Linux Release.