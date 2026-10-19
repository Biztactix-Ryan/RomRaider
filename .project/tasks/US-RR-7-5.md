---
archived: false
assignee: claude
claimed_at: '2026-10-19T08:43:40.927645+00:00'
claimed_by_run: orch-2026-10-19-acf8
created: '2026-10-19'
depends_on:
- US-RR-1-5
id: US-RR-7-5
points: 3
status: done
story_id: US-RR-7
tags: []
title: Define capability token types with sealed minting
updated: '2026-10-19'
---

rr-core::capability: RomWrite, EcuMemoryWrite{ecu_id, range}, EcuReset; private fields, no Default/Clone/Deserialize; sealed Confirmation trait implemented only by rr-app/rr-cli confirmation modules.