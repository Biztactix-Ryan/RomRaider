---
acceptance_criteria:
- The nightly workflow publishes a pre-release with tarball, AppImage, .deb and SHA256SUMS
- A CI job installs the .deb in a clean Ubuntu container and runs romraider-cli --version
  successfully
- The AppImage runs romraider-cli --version on a clean Ubuntu 22.04 container
- Release artifacts never contain ROM images or fixture data (artifact content check)
created: '2026-10-19'
depends_on:
- US-RR-2
epic_id: EPIC-RR-9
id: US-RR-38
points: 5
priority: must
status: active
tags:
- m0
- release
- linux
title: 'Release skeleton: nightly CLI tarball, AppImage and .deb'
updated: '2026-10-20'
---

As a Linux user, I want installable nightly builds from day one, so that packaging is proven continuously rather than bolted on at the end.

- release.yml with cargo-dist: nightly schedule plus tag trigger; romraider-cli tarball and SHA-256 checksums.
- AppImage (appimagetool) and .deb (cargo-deb) for x86_64.
- Desktop entry and icon placeholders.
- Published as a GitHub pre-release on the fork.
- Binary names romraider-rs / romraider-cli.

Test tiers: smoke (install + run --version in a container). Refs: INFRASTRUCTURE.md Linux Release, D-022.