---
acceptance_criteria:
- Killing the process at any point during a settings save leaves the previous settings
  intact (fault-injection test, MF-35)
- A second launch with a path containing spaces and unicode opens it in the running
  instance (MF-36); the IPC endpoint is not accessible to other users (permission
  test)
- Java settings.xml import produces the expected typed settings for a hand-checked
  fixture and never modifies the source file
- No settings file in the current working directory is ever read implicitly (MF-35)
created: '2026-10-19'
depends_on:
- US-RR-7
- US-RR-22
- US-RR-12
epic_id: EPIC-RR-8
id: US-RR-59
points: 8
priority: should
status: backlog
tags:
- m4
- ui
title: 'App shell: settings, single-instance, diagnostics'
updated: '2026-10-19'
---

As a user, I want the desktop app to start fast, remember my settings safely, and open files from my file manager into the running instance.

- eframe app with AppState (no globals). Confirm D-020 (egui) before starting.
- Typed settings (EditorSettings/LoggerSettings/UiSettings) through Layered: defaults → file → CLI; TOML, atomic writes. One-time importer for Java settings.xml (spec from the legacy catalogue).
- Single-instance via a per-user socket or named pipe with JSON args.
- Diagnostics panel.
- X11 and Wayland, glow renderer.

Test tiers: T0, T1, T10. Closes MF-35, MF-36, MF-43. Refs: D-014, D-015, D-020.