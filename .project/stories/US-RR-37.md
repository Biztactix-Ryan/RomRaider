---
acceptance_criteria:
- Using a pty pair (Linux) / com0com-style harness, echo bytes are stripped and an
  echo mismatch is reported as a link error, never passed up as response data
- Removing the device mid-transaction yields a Link error within the configured deadline,
  with no hang or panic (EC-18)
- After replug on a different /dev/ttyUSBn the link reconnects by stable ID (EC-19c)
- The FTDI latency timer value is detected on Linux and a Warning diagnostic is raised
  if it is above 1 ms
- 'HIL: opening the KKL cable at 4800 8N1 against the Liberty succeeds (recorded in
  the HIL report)'
created: '2026-10-19'
depends_on:
- US-RR-33
- US-RR-28
epic_id: EPIC-RR-6
id: US-RR-37
points: 8
priority: must
status: backlog
tags:
- m2
- comms
- kkl
- linux
title: Serial Link and KKL cable profile
updated: '2026-10-19'
---

As the owner of a 2004 AU Liberty with a KKL cable, I want the serial link to handle K-line's quirks automatically, so that logging just works and a flaky cable never shows wrong data.

- serialport-based Link: 4800 8N1, no flow control.
- K-line echo handling: own transmitted bytes stripped and verified, with a mismatch reported as a bus error.
- Break signal for clearing the line, duration from the profile.
- KKL adapter profiles: FTDI FT232R (0403:6001) with latency_timer guidance (16 ms default → 1 ms); CH340 (1a86:7523) quirks.
- Port discovery by stable ID (/dev/serial/by-id, Windows device instance).
- Unplug and replug handled with no hang.
- Blocking I/O on a dedicated thread with Clock-based deadlines.

Test tiers: T0, T5 (via a loopback/pty harness), T9. Closes EC-18, EC-19c. Refs: VISION.md target, INFRASTRUCTURE.md Linux device access.