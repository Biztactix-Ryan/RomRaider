---
created: '2026-10-19'
id: EPIC-RR-7
points: null
priority: should
status: draft
tags:
- m3
- live
target_date: null
title: Live Data (rr-live)
updated: '2026-10-19'
---

M3. The SampleFrame pipeline with Option values, the derived-signal graph, wideband devices as profiles, the CSV writer/importer/playback producer, and role-based collectors (MAF, injector, dyno).

Success criteria:
- No silent zeros anywhere.
- Stale device data goes None after its TTL.
- Playback reproduces recorded sessions exactly.
- About 8 of the 13 legacy devices are pure profiles.
- Closes MF-09/10/24/26/30/31/32/33/34/42, EC-24..28.

Stories only for now; tasks are decomposed when M2 lands.