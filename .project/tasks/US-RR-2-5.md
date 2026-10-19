---
archived: false
assignee: null
claimed_at: null
claimed_by_run: null
created: '2026-10-19'
depends_on: []
id: US-RR-2-5
points: 3
status: todo
story_id: US-RR-2
tags: []
title: 'Write ci.yml: fmt, clippy -D warnings, nextest matrix'
updated: '2026-10-19'
---

.github/workflows/ci.yml: rustfmt --check, cargo clippy --workspace --all-targets -D warnings, cargo nextest on x86_64-linux, aarch64-linux (native arm runner or cross), x86_64-windows-msvc, i686-windows-msvc. Rust cache.