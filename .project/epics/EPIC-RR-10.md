---
created: '2026-10-19'
id: EPIC-RR-10
points: null
priority: could
status: draft
tags:
- m6
- breadth
target_date: null
title: Protocol & Import Breadth
updated: '2026-10-19'
---

M6. Everything beyond the target car:
- Links and codecs: J2534 Link, KWP2000, SSM over CAN, OBD-II, NCS (Nissan), DS2 (BMW), ELM327 Link
- Imports: XDF/VDF, ECUFlash XML
- The native OpenPort 2.0 Link spike (D-023)

Each is a codec or Link plus profiles reusing the existing concepts. A new concept needs a DECISIONS entry.

Success criteria:
- Each protocol has simulator + replay coverage and a fault matrix before it's considered supported.
- Imports pass cross-source agreement against RomRaider XML definitions.
- Closes MF-20/21, EC-19/22.

Stories only for now.