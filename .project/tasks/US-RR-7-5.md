---
archived: false
assignee: null
claimed_at: null
claimed_by_run: null
created: '2026-10-19'
depends_on: []
id: US-RR-7-5
points: 3
status: todo
story_id: US-RR-7
tags: []
title: Define capability token types with sealed minting
updated: '2026-10-19'
---

rr-core::capability: RomWrite, EcuMemoryWrite{ecu_id, range}, EcuReset; private fields, no Default/Clone/Deserialize; sealed Confirmation trait implemented only by rr-app/rr-cli confirmation modules.