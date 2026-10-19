---
acceptance_criteria:
- The rules file is generated from adapter profiles and a CI check fails if the committed
  file is out of date
- doctor detects each condition (EACCES, brltty, ModemManager, missing device, high
  latency timer) in tests using a fake sysfs/proc tree, with a snapshot of the advice
  text
- On a fresh Ubuntu 24.04 install with the .deb, the KKL cable opens without any manual
  permission change (HIL report)
- The pkexec install path displays the rule file contents before asking for elevation
created: '2026-10-19'
depends_on:
- US-RR-37
- US-RR-28
epic_id: EPIC-RR-9
id: US-RR-53
points: 8
priority: must
status: backlog
tags:
- m2
- linux
- packaging
- kkl
title: udev rules and the doctor command
updated: '2026-10-19'
---

As a Linux user plugging in a KKL cable, I want it to just work, and if it doesn't, to be told exactly why and how to fix it.

- packaging/linux/60-romraider.rules generated from adapter profiles (single source of truth): `TAG+="uaccess"`, `ENV{ID_MM_DEVICE_IGNORE}="1"`, FTDI `latency_timer=1`. FTDI 0403:6001 and CH340 1a86:7523 first.
- `romraider-cli doctor` (and an in-app equivalent later) detects: EACCES, brltty holding the device, ModemManager on the port, adapter not enumerated, latency timer > 1 ms, missing J2534 library.
- Offers fixes; rule install goes via pkexec, showing file contents first.

Test tiers: T0, T9. Closes EC-19a, EC-19b. Refs: INFRASTRUCTURE.md Device access, SECURITY.md Linux device access.