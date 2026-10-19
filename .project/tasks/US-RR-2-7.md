---
archived: false
assignee: claude
claimed_at: '2026-10-19T09:12:44.455888+00:00'
claimed_by_run: orch-2026-10-19-acf8
created: '2026-10-19'
depends_on:
- US-RR-2-5
id: US-RR-2-7
points: 2
status: done
story_id: US-RR-2
tags: []
title: Add insta pending-snapshot check and i686 target setup
updated: '2026-10-19'
---

CI step failing on pending .snap.new files (INSTA_UPDATE=no, cargo insta test --check). Install i686-pc-windows-msvc target and run tests there.