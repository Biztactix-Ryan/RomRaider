---
archived: false
assignee: claude
claimed_at: null
claimed_by_run: null
created: '2026-10-19'
depends_on: []
id: US-RR-11-5
points: 3
status: done
story_id: US-RR-11
tags: []
title: Implement xtask fetch-corpus
updated: '2026-10-20'
---

Fetch from the private backup https://github.com/Biztactix-Ryan/romraider-stock-roms first (needs auth in CI: FIXTURE_CORPUS_TOKEN or GITHUB_TOKEN with repo access), falling back to upstream samuelw9999/RomRaider @639f3c73. Fetch into RR_CORPUS_DIR (default ~/.cache/romraider-rs/corpus), verify against the manifest, and stay idempotent.