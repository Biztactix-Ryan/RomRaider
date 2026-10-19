---
created: '2026-10-19'
id: EPIC-RR-6
points: null
priority: must
status: draft
tags:
- m2
- comms
- ssm
- safety
target_date: null
title: 'Comms Core: SSM over K-line (KKL)'
updated: '2026-10-19'
---

M2. Link trait + .rrtrace + Replay, the transaction engine (Clock timing, framing, echo, recovery, quirk Rules), the ECU simulator with Rule-based fault injection, the SSM K-line codec, the Serial Link + KKL cable profile, ECU write gating, romraider-cli log/capture/replay/scrub, read-only HIL on Ryan's 2004 AU Liberty NA, and a spike on reading the ROM over SSM.

Success criteria:
- Read-only logging works on the Liberty via the KKL cable.
- Bad or missing responses yield None, never stale data.
- The simulator proves zero unauthorised writes.
- The fault matrix is covered.
- Real traces are committed (scrubbed) as replay fixtures.
- Closes MF-08/27/28/29, EC-15/16/18/19c/21.