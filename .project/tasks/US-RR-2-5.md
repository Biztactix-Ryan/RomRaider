---
archived: false
assignee: claude
claimed_at: '2026-10-19T08:46:33.331598+00:00'
claimed_by_run: orch-2026-10-19-acf8
created: '2026-10-19'
depends_on:
- US-RR-1-5
id: US-RR-2-5
points: 3
status: done
story_id: US-RR-2
tags: []
title: 'Write ci.yml: fmt, clippy -D warnings, nextest matrix'
updated: '2026-10-19'
---

.github/workflows/ci.yml: rustfmt --check, cargo clippy --workspace --all-targets -D warnings, cargo nextest on x86_64-linux, aarch64-linux (native arm runner or cross), x86_64-windows-msvc, i686-windows-msvc. Rust cache.