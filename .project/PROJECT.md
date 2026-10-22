# RomRaider-RS

A ground-up Rust rewrite of RomRaider (ECU ROM editor + data logger). It has a unified, data-driven core and a fixture-first test strategy.

- Repo: https://github.com/Biztactix-Ryan/RomRaider (fork of RomRaider/RomRaider). Work happens on branch `rust-rewrite`.
- Upstream remote: `upstream` → https://github.com/RomRaider/RomRaider. Pull Java fixes into `legacy/` only.

## Architecture

See ARCHITECTURE.md for the concept catalogue (C1–C16) and the crate map. This section is the practical summary.

### Tech Stack

- **Language**: Rust (stable, pinned in `rust-toolchain.toml`; edition 2024).
- **UI**: egui / eframe, with `egui_plot` for charts. The 3D table surface uses a custom egui_wgpu paint callback, or `three-d` (TBD in M4).
- **Database**: none. Data lives in files: ROM images, XML definitions, TOML profiles and settings, CSV logs, `.rrtrace` traces.
- **Key Libraries**:
  - Parsing: `quick-xml` (streaming definition reads), `roxmltree` (XDF), `serde` with `toml`/`serde_json`
  - Hardware: `serialport`, `libloading` (J2534 DLLs), `winreg` (J2534 discovery on Windows)
  - Errors and logging: `thiserror`, `anyhow` (edges only), `tracing`
  - Concurrency: `crossbeam-channel`
  - Platform: `directories`, `clap` (cli), `csv`
  - Formula parser: in-house Pratt parser (JEP-compatible), no external crate.
- **No Java** anywhere in the build, tests, tooling or CI (D-024). `legacy/` is read-only reference source.

### Components

| Path | What |
|------|------|
| `crates/rr-core` | Pure concept types: Layered/Resolver, Pattern, Encoding, Expr/Conversion, Units, Framing, Rule, Clock, Diagnostic |
| `crates/rr-defs` | Definition and profile readers (RomRaider XML, logger XML, XDF, TOML) → resolved typed definitions |
| `crates/rr-rom` | Byte image, Patch journal, N-D Table, checksums, clipboard codec, diff |
| `crates/rr-link` | Link trait + Serial / J2534 / ELM327 / Replay / Sim, Trace format |
| `crates/rr-comms` | Transaction engine, quirk application, protocol codecs (SSM, KWP2000/NCS, OBD, DS2), write gating |
| `crates/rr-live` | Signals, SampleFrames, devices, derived-signal graph, collectors, CSV writer and playback |
| `crates/rr-testkit` | Fixture loaders, snapshot helpers, proptest strategies, ECU simulator wiring, VirtualClock |
| `crates/rr-cli` | Headless tool and integration-test surface |
| `crates/rr-app` | Desktop app (editor + logger) |
| `packaging/` | Linux packaging: udev rules (generated from adapter profiles), `.desktop` file, icons, AppImage recipe, AUR PKGBUILD, `install.sh`; Windows MSI config |
| `fixtures/corpus-manifest.toml` | Pinned manifest of the external stock-ROM and definition corpora: path, size, SHA-256, ROM ID. The ROMs themselves are never committed. |
| `fixtures/kat` | Hand-authored known-answer fixtures (TOML), each expected value with a stated source |
| `fixtures/public` | Committed synthetic ROMs, definitions, traces and golden outputs (no proprietary firmware) |
| `~/RomRaider-private` (outside the repo) | Private fixture corpus: real ROMs and traces, path set by `RR_FIXTURES_DIR`. Never inside the working tree; the loader refuses one that is. |
| `legacy/` | Original Java RomRaider: read-only reference, never built or run by the project |

### Data Flow

**Editor path:**
```
ROM file ──bytes──▶ rr-rom Image
definition XML ──rr-defs readers──▶ Layered docs ──resolve──▶ Definition (+Diagnostics)
ROM-ID Pattern match picks the Definition
Table = view(Image, Encoding, Conversion, axes)
edit ──▶ Patch ──▶ Journal (undo/redo, modified markers)
save ──▶ checksum Patches ──▶ verify ──▶ atomic write + .bak
```

**Logger path:**
```
LinkProfile + quirk Rules resolved for (protocol, adapter, ECU family, ECU ID)
rr-comms engine ──transact──▶ Link (Serial/J2534/ELM/Replay/Sim) ──Trace──▶ .rrtrace
codec decodes response ──Encoding+Conversion──▶ SampleFrame
devices (stream Links) + derived graph ──▶ SampleFrame
SampleFrame ──bounded channel──▶ UI | CSV writer | Collectors | table overlay
```

## Key Decisions

Full log in DECISIONS.md. Headline decisions:
- Rewrite in Rust; Java kept in `legacy/` as read-only reference; no Java in tooling or tests — 2026-10-19
- Unified concept catalogue; a new concept requires a decision entry — 2026-10-19
- Fixture-first, multi-tier testing with mutation and coverage gates — 2026-10-19
- Quirks and timing as layered data (Rules), not code — 2026-10-19
- Byte image + Patch journal is the only source of truth — 2026-10-19
- Read-only by default; capability-gated writes — 2026-10-19

## Dependencies

| Dependency | Purpose | Why |
|-----------|---------|-----|
| eframe/egui, egui_plot | UI, charts | Immediate mode suits grid-heavy editing. Single binary, no web runtime. |
| serialport (no default features) | Serial and K-line | De facto standard, supports break signalling. Built without libudev, so there's no Linux runtime dependency. |
| nusb | Native OpenPort 2.0 USB Link (D-023) | Pure-Rust USB, no libusb; Linux, macOS, Windows |
| cargo-dist, cargo-deb, cargo-generate-rpm, cargo-zigbuild, appimagetool | Release artifacts | One pipeline producing MSI, AppImage, .deb, .rpm, tarball and AUR; glibc 2.28 baseline |
| libloading (+ winreg) | J2534 PassThru DLLs | Thin FFI. Registry-based discovery replaces JNA platform.jar. |
| quick-xml, roxmltree | Definition / XDF reading | Fast streaming for large definition files; DOM for XDF |
| serde, toml, serde_json | Profiles, settings, fixtures, traces | Typed, schema-versioned |
| thiserror / anyhow / tracing | Errors, logging | Typed library errors; structured logs |
| crossbeam-channel | Thread communication | Bounded channels, select |
| directories | Settings location | Correct per-OS paths |
| csv | Log writing and import | RFC 4180 |
| insta, proptest, libfuzzer-sys | Snapshot, property, fuzz tests | See Testing Strategy |
| cargo-nextest, cargo-llvm-cov, cargo-mutants, cargo-deny | Test runner, coverage, mutation, supply chain | CI gates |

Dropped Java dependencies:
- Java3D, Graph3d and vecmath
- JFreeChart and jama/jamlab (polyfit becomes about 30 lines)
- args4j (unused) and JNA/platform.jar
- com4j and phidget21 (deferred)
- log4j 1.2

## Development Setup

**Prerequisites:**
- Rust via rustup (toolchain auto-selected by `rust-toolchain.toml`).
- Linux: nothing extra for the core (serialport is built without libudev). The GUI needs the usual X11/Wayland and OpenGL dev headers (`libxkbcommon-dev`, `libwayland-dev`, `libgl1-mesa-dev` on Debian/Ubuntu). Packaging tools (`cargo-deb`, `cargo-generate-rpm`, `cargo-zigbuild`, `appimagetool`) are only needed to build release artifacts.
- Optional:
  - `cargo-nextest`, `cargo-llvm-cov`, `cargo-mutants`, `cargo-fuzz` (nightly)

**Commands:**
```
cargo xtask fetch-corpus              # fetch + verify external stock-ROM/definition corpora into RR_CORPUS_DIR
cargo nextest run                     # all tests (public fixtures + external corpus if fetched)
RR_FIXTURES_DIR=~/RomRaider-private cargo nextest run --workspace --features private-fixtures
cargo insta review                    # review snapshot changes
cargo llvm-cov nextest                # coverage
cargo mutants -p rr-core -p rr-rom    # mutation testing (safety crates)
cargo +nightly fuzz run <target>      # fuzz a reader/parser
cargo xtask gen-synthetic             # regenerate synthetic ROMs from the reference encoder
```

**Env vars** (names only):
- `RR_LOG` sets the tracing filter.
- `RR_CORPUS_DIR` is the external stock-ROM/definition corpus cache.
- `RR_FIXTURES_DIR` is the private fixture corpus (`~/RomRaider-private`).
- `RR_HIL_PORT` / `RR_HIL_J2534` set the hardware-in-loop port or J2534 library; HIL tests are skipped unless set.
- `RR_J2534_LIB` overrides J2534 DLL discovery.

## Testing Strategy

Ten tests on software that can brick an ECU is unacceptable. Testing is a first-class deliverable, built in M0 *before* feature code. Every tier runs in CI unless noted.

| Tier | What | Tooling | Applies to | Gate |
|------|------|---------|-----------|------|
| **T0 Unit** | Behaviour of each function and type | `#[test]`, nextest | All crates | Must pass |
| **T1 Property** | Invariants over generated inputs: Encoding round-trip for every storage/endian/bitfield combination; Conversion `to_raw(to_real(x)) == x` within tolerance where invertible, error where not; Resolver idempotence, cycle detection, omit semantics; Patch journal undo∘redo = identity; Framing split/join | `proptest` | rr-core, rr-rom, rr-comms | Must pass |
| **T2 Known-answer & invariants** | Expected values from **Java-free oracles** (D-024): (a) hand-checked KAT fixtures, each with a stated source; (b) an independent, deliberately naive reference encoder/decoder in rr-testkit that shares no code with rr-core and generates synthetic ROMs from known values; (c) stock-ROM invariants: every stock ROM verifies, fix-checksums is a no-op, a byte flip fails verify, and re-fixing changes only checksum bytes; (d) published standard vectors (SAE J1979 PIDs, ISO 9141/14230/15765 timing, checksum and CRC reference vectors); (e) cross-source agreement, e.g. RomRaider XML vs XDF for the same ROM; (f) real car traces | `rr-testkit` KAT loader, `insta` snapshots, private corpus | rr-core, rr-defs, rr-rom, rr-comms, rr-live | Must pass. A deliberate departure from legacy behaviour is recorded in the Must-Fix ledger. |
| **T3 Fuzz** | No panic, hang or unbounded allocation on hostile input | `cargo-fuzz` targets: ECU XML, logger XML, XDF, TOML profiles, expr parser, every Framing, every protocol response parser, `.rrtrace` reader | rr-defs, rr-core, rr-comms, rr-link | Nightly CI job for 10 min per target; crashes become regression tests |
| **T4 Replay** | Recorded `.rrtrace` sessions replayed through the engine on a VirtualClock. Asserts the same requests, the same decoded SampleFrames and the same quirk applications at the same virtual times. | Replay Link + VirtualClock | rr-comms, rr-live | Must pass. Every bug found on a real car adds a trace. |
| **T5 Simulator** | Protocol codecs run in reverse as a simulated ECU serving a ROM/RAM image. Fault injection uses the quirk Rule engine: dropped/late/corrupt bytes, missing echo, ECU going silent, baud change, partial frames, adapter unplug. | Sim Link + Rules + VirtualClock | rr-comms, rr-live | Must pass. Every protocol × fault matrix entry is covered. |
| **T6 Safety invariants** | No ECU write without a capability token. No ROM save with a failing checksum unless overridden. Saves are atomic and leave a `.bak`. Unknown encoding is a load error. An out-of-range edit is rejected, never wrapped. | Dedicated test module + `#[deny]`-style lints; the Sim records attempted writes | rr-rom, rr-comms, rr-app | Must pass, release-blocking |
| **T7 Mutation** | Tests actually detect broken logic | `cargo-mutants` | rr-core::encoding, rr-core::expr, rr-rom (journal, checksum, table write path), rr-comms write gating | 0 surviving mutants in gated modules (justified exclusions listed in `mutants.toml`) |
| **T8 Coverage** | Visibility, not a target to game | `cargo-llvm-cov` | All | Line coverage ≥ 90% for rr-core / rr-rom / rr-comms; must not regress on PRs |
| **T9 Hardware-in-loop** | Real adapter + real or bench ECU, **read-only** by default. Write tests only on a bench ECU, behind an extra flag. | `RR_HIL_*`, self-hosted runner (TBD) | rr-link, rr-comms | Manual before releases; scheduled once a bench exists |
| **T10 UI smoke** | App starts, opens a fixture ROM, edits a cell, undoes, saves to a temp dir | egui_kittest | rr-app | Must pass |

**Rules:**
- Every Must-Fix and Edge-Case entry below names the test that proves it. An entry is not closed until that test exists and passes.
- A bug report gets a failing test (or a trace/fixture) before it gets a fix.
- Every expected value in a fixture states its source. Changing an expected value is a reviewed PR that explains why the old value was wrong.
- Comparing against the Java app is allowed only as an ad-hoc, manual check in a temp directory. Nothing from it is committed or gated on.
- Public CI uses only synthetic ROMs. Real ROMs live in the private corpus (see SECURITY.md).

## Must-Fix Bugs (from the Java codebase)

These are behaviours the Rust version must **not** reproduce. Java paths are relative to `legacy/src/main/java/com/romraider/`.

Priority levels:
- **P0:** can corrupt a ROM, write bad values to an ECU, or present wrong live data that could lead to engine damage.
- **P1:** wrong results or silent failure.
- **P2:** correctness or UX.

| ID | Pri | Area | Java behaviour (where) | Required behaviour | Proving test |
|----|-----|------|------------------------|--------------------|--------------|
| MF-01 | P0 | Checksum | E38 PCM `update` ignores the `calculated` flag. When the segment table fails its checks, `calculate` returns before loading any segment, so every value stays zero, `update` writes **nothing** and the ROM is saved with its old sums, reported as "0 of 12 updated"; only the open-time popup hints at it. A start or end word of `80000000` or more is negative after the cast and passes the range checks, and a segment past the image throws, aborting the open or save. Corrections are also counted per pass of the two-pass loop, so it can report 18 of 12 (`maps/checksum/ChecksumE38PCM.java:71-97,119-142`). | Only write sums for validated segments. If any segment fails (including a negative or out-of-image range), return an error and write nothing. The two-pass dependency becomes an explicit ordered spec. | T2 fixture + T6 "invalid segment → no patch" + T7 (LEG-CHK-10, LEG-CHK-11) |
| MF-02 | P0 | Encoding | int24 and int64 widths fall through the decode switch and read as 0. Editing writes garbage (`xml/RomAttributeParser.java`). | Every storage width is supported or is a load error. | T1 round-trip all widths; T6 unknown storage → Diagnostic error |
| MF-03 | P0 | Editor | Overlap map keyed by `index*storageType` is wrong for MOVI20 (20) and float (99), so overlapping tables desync or overwrite each other (`maps/DataCell.java:343`). | Overlap falls out of the byte image: cells are views, and edits are Patches. | T1 overlapping-table edit property; T2 |
| MF-04 | P0 | Encoding | `Table.memModelEndian` is static, shared across every open ROM (`maps/Table.java:40`). | Endian belongs to the Encoding, per definition. | Test: open a BE and an LE ROM at once and decode both |
| MF-05 | P0 | Conversion | The missing-`to_byte` inverse is a binary search that assumes monotonic functions and overflows for 4-byte storage. Its cache returns a different value than an uncached call (`maps/Scale.java:87,135,139`). | Exact inverse for linear forms. Otherwise a bounded solve over the valid raw range, and an error if the function isn't invertible there. Deterministic. | T1 inverse property incl. decreasing and non-monotonic; T7 |
| MF-06 | P0 | Definitions | Signedness is only inherited while `storageType<1`, so a child definition can't switch int/uint (`xml/TableScaleUnmarshaller.java:153`). | Resolver applies every field override uniformly (C1). | T1 resolver override property; T2 fixture with a child sign flip |
| MF-07 | P0 | Definitions | `filterFoundRomTables` returns on the first omitted table, so later tables lose their addresses (`xml/TableScaleUnmarshaller.java:332`). | `omit` removes only that table. | Resolver test: omit in the middle of a list |
| MF-08 | P0 | Logger | On a bad serial response, the *previous good response* is copied in and shown as live (`io/serial/connection/SerialConnectionManager.java:96-101`). | A bad or missing response gives `None` values plus a link-health event. Stale data is never shown as live. | T5 corrupt-frame fault; UI marks stale |
| MF-09 | P0 | Logger | Missing parameter values silently become 0.0 (`logger/ecu/comms/query/ResponseImpl.java`). | `Option<f64>` end to end; gaps are visible in the UI and CSV. | T4/T5 dropped-param assertions |
| MF-10 | P0 | Devices | TE wideband computes a checksum but still uses the data when it fails. Both branches of the sequence-wrap check are identical (`logger/external/te/io/TERunner.java:86,93-100`). | Framing `Sync{checksum}` drops bad frames. Sequence wrap is handled once in the Framing. | T1 framing property; T5 corrupt device stream |
| MF-11 | P0 | Security | Checksum classes are loaded from the definition directory via URLClassLoader, so a downloaded definition pack can execute code (`maps/checksum/ChecksumFactory.java:77`). | No runtime code loading. Checksums come from a ChecksumSpec plus a compiled-in named registry. | Unknown algorithm → Diagnostic error |
| MF-12 | P1 | Editor | `Table3D` redeclares `data`, hiding the parent field, so inherited ops work on a dummy 1×1 array (`maps/Table3D.java:36`). | One N-D Table type (C11). | T2 3D table size/copy/compare fixtures |
| MF-13 | P1 | Editor | `getMaxReal = f(maxBin)` is wrong for decreasing scalings (`maps/Table.java:444`). | Range = min/max of f over the raw range endpoints (and extrema for Expr). | T1 decreasing-scaling property |
| MF-14 | P1 | Editor | 2D/3D interpolation works on raw values, which is wrong for non-linear scalings. | Interpolate in real space, then encode. | Property: interpolate then decode gives linear real values |
| MF-15 | P1 | Editor | Float min/max range has signed and unsigned swapped (`maps/DataCell.java:143`). | Range derives from Encoding. | T1 |
| MF-16 | P1 | Definitions | Table-local scalings register as global scaling bases (`xml/TableScaleUnmarshaller.java:461`). | Scoped scaling namespaces. | Resolver test: local name doesn't leak |
| MF-17 | P1 | Definitions | ROM `base` inheritance only works within one file, and cycles are caught as `StackOverflowError`. | Cross-file resolution, explicit cycle Diagnostic. | Resolver cycle and cross-file tests |
| MF-18 | P1 | Definitions | Table `base` uses a Java-serialization deep copy, which probably clones the whole ROM object graph (`xml/TableScaleUnmarshaller.java:90`). | Shared base via `Arc`, override on resolve. | Perf test: 1000-table definition resolves < 200 ms (enforced at 5000 tables by the `perf-budget` CI gate, US-RR-34-4) |
| MF-19 | P1 | Checksum | The legacy Subaru checksum is triggered by a table *named* "Checksum Fix", with edit-stamp offsets +204/+207 hard-coded (`maps/Rom.java:337,455`, `maps/DataCell.java:211,482`). | ChecksumSpec in the definition family profile. | T2 Subaru fixture |
| MF-20 | P1 | Import | VDF import drops any scale factor between 0.99 and 1.01 (`xml/ConversionLayer/VDFConversionLayer.java:211`). | Keep exact factors. | T2 VDF fixture |
| MF-21 | P1 | Import | XDF problems: equations fixed up by string replace (X→x, ","→"."); REGION filesize created but never appended; float XDFs ignored; `lsbfirst` default and 0x02 flag use opposite polarity (`xml/ConversionLayer/XDFConversionLayer.java:275,278,507,514`). | Parse into the Expr AST; support float; one polarity rule; full region. | T2 + T3 fuzz on XDF |
| MF-22 | P1 | Editor | ROM compare is O(n²) with inconsistent equality (1D ignores the name) and compares raw values regardless of scaling (`swing/CompareImagesForm`). | Diff = Patches (C10), compared both raw and real. | Diff property tests |
| MF-23 | P1 | Editor | `Rom.populateTables` removes from the map while iterating, and uses NPE as control flow (`maps/Rom.java:320-324`). | Diagnostics, no exceptions as control flow. | Covered by resolver tests |
| MF-24 | P1 | Logger defs | Derived params with missing dependencies silently vanish; derived params can't use external sensors (`logger/ecu/definition/xml/LoggerDefinitionHandler.java:336`). | Derived graph over Signals, topologically sorted; a missing dependency gives a Diagnostic. | Derived-graph tests incl. an external dependency |
| MF-25 | P1 | Expr | Two JEP dialects: the map overload lacks the standard functions and `BitWise`. Global `synchronized` cache of 32 entries; derived params run `String.replace` per sample (`util/JEPUtil.java:47,63`; `EcuDerivedParameterConvertorImpl.java:63`). | One parser, one dialect, parsed once at load. | T1 + T2 conversion fixtures |
| MF-26 | P1 | Devices | External values have no timestamp or staleness check (`logger/external/core/ExternalDataConvertorImpl.java:48`). | Timestamped Signals with TTL; stale → `None`. | T5 device-unplug fault |
| MF-27 | P1 | Comms | Read timeout counts `sleep(1)` iterations, not wall-clock time. With Windows timer granularity the real timeout can be ~15× longer (`io/serial/connection/SerialConnectionManager.java:76-79`). | Deadlines from Clock (C9). | T4 VirtualClock timeout test |
| MF-28 | P1 | Comms | Magic `sleep(55L)` after the NCS ECU-ID request (it is ISO 14230 P3min), duplicated alongside J2534 `P3_MIN` config (`logger/ecu/comms/io/connection/NCSLoggerConnection.java:111`). | Timing from LinkProfile; every legacy sleep mapped to a named timing or quirk Rule. The 55 ms is ISO 14230-2's default P3min, `rr_core::timing::iso::iso14230_2::P3_MIN`, one of the P1–P4 defaults in `iso14230_2` backed by `fixtures/kat/timing-iso14230-2.toml` (ISO 14230-2:1999 4.5.1, Table 3). | T4 timing assertions; T2 `timing_kat` (constants vs. fixture) |
| MF-29 | P1 | Comms | `SSMProtocol.module` is a public static mutable field (`io/protocol/ssm/iso9141/SSMProtocol.java`). | Codecs are stateless or own their state. | Two concurrent sessions in the Sim |
| MF-30 | P1 | Logger | CSV header is rewritten mid-file; values are stored as locale-formatted strings; delimiter comes from a hard-coded list of 57 locales (`logger/ecu/ui/handler/file/FileUpdateHandlerImpl.java:47,84,108`). | Fixed schema per file, raw numbers, `.` decimal, rotate on schema change. | CSV round-trip property; playback equals source |
| MF-31 | P1 | Logger | MAF/Injector/Dyno tabs hard-code SSM param IDs (`"P8"`, `"E33"`) (`logger/ecu/ui/handler/maf/MafUpdateHandler.java:35-142`, `ui/tab/injector/InjectorControlPanel.java:77-90`, `ui/tab/dyno/DynoControlPanel.java:107-112`). | Roles (C13). | Collector tests per protocol |
| MF-32 | P1 | Logger | Selecting a parameter opens a serial port as a side effect (`logger/external/core/ExternalDataImpl.java:~90,108`). | Device connections are ref-counted by the sampling engine. | Test: selection doesn't touch the Link |
| MF-33 | P1 | Devices | Device ID is built from the display name (`"X_"+name`), so renaming breaks saved profiles (`ExternalDataImpl.java:99`). | Stable IDs in device profiles. | Profile load test |
| MF-34 | P1 | Threading | Non-volatile stop flags (`QueryManagerImpl.java:86` is even static; about 9 `*Runner`s). The data thread reads Swing widgets (`PlainGaugeStyle.java:96`) and highlights tables off the EDT (`TableUpdateHandler.java:61`). Dispatcher holds a lock while running handlers, over an unbounded queue (`AsyncDataUpdateHandler.java:50,64`). | Channels + cancellation tokens; the UI only reads frames. | Sim stop/start stress test |
| MF-35 | P1 | Settings | Non-atomic save over `settings.xml`, checked only afterwards; a `settings.xml` in the CWD silently wins (`util/SettingsManager.java:68-72,120-127`). Static fields are set through instance setters (`Settings.java:212,235,240`). | Typed TOML, atomic write, one location. | Save interrupted mid-write → old file intact |
| MF-36 | P1 | Security | Single instance uses an unauthenticated localhost TCP port 23272: racy probe, args split on spaces (`EditorLoggerCommunication.java`). | Per-user local socket/named pipe carrying JSON args. | Path-with-spaces test |
| MF-37 | P2 | Editor | `setUserLevel` clamps the parameter after it has already been assigned (`maps/Table.java:677`). | n/a (typed enum) | — |
| MF-38 | P2 | Editor | `TableBitwiseSwitchView` is never created because the `instanceof TableSwitch` check comes first (`ECUEditor.java:462`). | One Table kind enum, exhaustive match. | UI smoke for a bitwise switch |
| MF-39 | P2 | Editor | `multiply` goes through a string with a locale comma hack; `increment` retries recursively, doubling the step (`maps/DataCell.java:582`). | Numeric ops in real space with explicit step rules. | T1 |
| MF-40 | P2 | Editor | A bad scaling raises a popup from model code (`maps/Table.java:558`). Every ROM open re-parses every definition file. | Diagnostics; cached ROM-ID index. | Diagnostic test; perf test (ROM-ID lookup < 5 ms, `perf-budget` CI gate, US-RR-34-4) |
| MF-41 | P2 | Logger | Dash warning thresholds are not saved in the profile (TODO, `logger/ecu/EcuLogger.java:208`). | Part of the profile schema. | Profile round-trip |
| MF-42 | P2 | Dyno | `KPH_2_MPH = 1.609344` is actually km per mile, so the name is inverted (also `logger/car/util/Constants`). | Units registry (C5). | Units tests |
| MF-43 | P2 | Startup | A missing i18n bundle gives null and a crash (`ECUExec.java:65`). | Strings embedded at compile time. | — |
| MF-44 | P2 | Tests | `XDFConversionLayerTest` points at `C:\Users\User\Downloads\…`, so it tests nothing. | Fixtures in-repo or in the private corpus. | — |
| MF-45 | P0 | Checksum | When a checksum manager's `configure` throws (missing or malformed attribute), the factory shows "Error loading Checksum Manager" but returns the manager anyway. On save, Motronic or `bytexor` then write their sum at image offset 0–1 or 0; Nissan throws on open. `type.toUpperCase()` is locale-dependent (`maps/checksum/ChecksumFactory.java:84-104`; `maps/checksum/ChecksumMOTRONICSINGLE.java:44-51,79-80`; `maps/checksum/ChecksumBYTEXOR.java:43-47,72`). | A ChecksumSpec with a missing or malformed parameter is a load Error; a partly configured checksum never runs. Algorithm names match locale-independently. | LEG-CHK-13 (US-RR-35-17) |
| MF-46 | P0 | Checksum | Save never re-validates checksums and has no consent path. An image the open-time check flagged as corrupt is recalculated on every save without asking. A locked Subaru "Checksum Fix" table is never recalculated (`isButtonSelected()` is always true for a `Table1D`, so its confirmation is unreachable) and its failing sums are saved silently. Ordering faults reach the file unnoticed: `alt2` reading stale `sumt`/`xort`, a Motronic `loc` inside its own range, a Subaru entry covering a later entry's `diff`, a manager writing inside another's range (`maps/Rom.java:417-437,474-488,590-601`; `maps/Table1D.java:79-82`; `maps/checksum/CalculateALT2.java:80-85`). | Every saved image re-validates every checksum after the patches apply. A failing checksum blocks the save unless the user explicitly overrides it, and the override is journaled (SECURITY.md `RomWrite`). | LEG-CHK-14 (US-RR-45-10); LEG-CHK-06, LEG-CHK-15 (US-RR-35-15) |
| MF-47 | P1 | Checksum | Subaru "Checksum Fix" table faults are silent. A `sizey` that is not a multiple of 12, or an image-controlled entry out of range, throws and the table is removed, so saves stop fixing the Subaru checksum with no message. A disabled first entry reports "all disabled" without checking the rest, while the save recalculates them; a disabled entry whose `diff` is not `5AA5A55A` is rewritten (`maps/RomChecksum.java:49-99`; `maps/Rom.java:312-318,330-332`). | A malformed Subaru checksum table is a load Error naming the table. Every entry is validated, and a disabled entry is left as it is. | LEG-CHK-08 (US-RR-35-16) |
| MF-48 | P0 | Checksum | Save mutates the open image before writing. The edit stamp, Subaru `diff`s and manager updates go into the in-memory image, so if a later step throws or the write fails, the open ROM keeps them with nothing saved. "Increase/Decrease ROM size" saves checksums computed before the block move (`maps/Rom.java:441-490`; `swing/ECUEditorMenuBar.java:501-514,624-664`). | Save computes checksum Patches on a copy and commits them to the open image only after the atomic write succeeds. Any image transform (resize) recomputes checksums on the transformed image. | LEG-CHK-14 (US-RR-45-10) |
| MF-49 | P0 | Definitions | Inherited addresses go wrong. When a `<table>` or axis changes an existing table and its name is not in the address filter, the default `storageaddress` is the inherited address printed in decimal and parsed as hex (base `0x2000` becomes `0x8192`). This hits every axis override without its own address, a child naming a table in different case, and tables lost to MF-07. The lookup also runs for axes by axis name, so an axis named like a top-level table takes that table's address (`xml/TableScaleUnmarshaller.java:171-179,233,248,262,317-345`). | An override that sets no address keeps the inherited one; axes resolve under their parent table, never by a global name (C1). | LEG-DEF-10 (US-RR-39-7) |
| MF-50 | P0 | Definitions | A child element resets inherited fields it does not set. Every `<scaling>` sets `to_byte` from its attribute or to none, so a scaling with `base`, or a units-only override, loses the base's `to_byte` and falls back to the MF-05 search. Every `<table>` without `dataLayout` resets it to `DEFAULT`, so a child of a Bosch-subtract table decodes with the wrong layout. The reverse also occurs: `beforeram` can be set but never cleared, so `beforeram="false"` keeps an inherited `true` (`xml/TableScaleUnmarshaller.java:145-150,442`; `maps/Scale.java:196-201`). | Resolver applies every field override uniformly: an unset field is inherited (C1, as MF-06). | LEG-DEF-20, LEG-DEF-22 (US-RR-39-9) |
| MF-51 | P1 | Definitions | A `base` that names nothing is silent. A scaling `base` that is unknown, or names a `<scalingbase>` later in the file, keeps the starting scale: the scaling vanishes behind the built-in raw scale or acts as a plain override. A table `base` copy returns `null` and the element is read as a new table (replacing, skipping or dropping it); the popup meant for this case is unreachable (`xml/TableScaleUnmarshaller.java:87-140,415-440`; `util/ObjectCloner.java:71-83`). | An unresolved `base` is an Error Diagnostic at the element; forward references within a file resolve. | LEG-DEF-09 (US-RR-39-7); LEG-DEF-20 (US-RR-39-9) |
| MF-52 | P0 | Definitions | The `<memmodel endian>` override lives in the reader and is set as a side effect of reading any `<romid>`, including those scanned to find a base by name. So a base without `<memmodel>` takes the endian of the last earlier `<rom>` in the file that had one; it applies only to tables read after the `<romid>`; a `<memmodel>` without `endian` clears it (`xml/DOMRomUnmarshaller.java:118-119,275-278`; `xml/TableScaleUnmarshaller.java:59-63,162-170`). Sibling of MF-04. | Memory-model endian is a field of the resolved ROM definition, inherited like any other (C1), and applies to every table of that ROM. | LEG-DEF-12 (US-RR-39-8) |
| MF-53 | P1 | Definitions | Checksums stack along the inheritance chain: each `<checksum>` of each level adds a manager, so a child cannot replace or remove its base's checksum and both run on save. An unknown checksum type adds a `null` manager that `validateChecksum` dereferences, so the ROM fails to open after the popup (`xml/DOMRomUnmarshaller.java:185-188`; `maps/Rom.java:557-559,565-588`). | ChecksumSpecs resolve like other fields, with explicit replace and remove. An unknown algorithm is an Error Diagnostic (MF-11). | LEG-DEF-25 (US-RR-35-17) |
| MF-54 | P1 | Definitions | Bitwise-switch `<bit>`s append to the inherited list with no duplicate check, so re-listing bits in a child doubles the cells. A position at or beyond the storage width is clamped into a wrong mask with no message (`maps/TableBitwiseSwitch.java:46-53,69-75`; `maps/DataCell.java:98-103`). | Bits are keyed by position and override by name; a position outside the storage width is an Error Diagnostic. | LEG-DEF-18 (US-RR-39-8) |
| MF-55 | P1 | Definitions | One malformed value fails far more than its element. A malformed `internalidaddress`, `offset` or `filesize` in any `<romid>` before the match skips the whole file (a `filesize` shorter than two characters throws `StringIndexOutOfBoundsException`). A top-level `<table>` without `name`, an upper-case `0X`, or an address of `0x80000000` or more throws and aborts the whole ROM load (`xml/DOMRomUnmarshaller.java:104-131,170`; `xml/RomAttributeParser.java:238-250`; `util/HexUtil.java:161-163`; `editor/ecu/OpenImageWorker.java:192-194,238-244`). | A bad value is an Error Diagnostic at its line and column. Only its element is disabled; every other ROM and table resolves (EC-02). | LEG-DEF-02 (US-RR-39-10); LEG-DEF-24 (US-RR-39-9) |
| MF-56 | P0 | Definitions | ROM-ID matching can pick the wrong definition. Matching uses only the `<rom>`'s own first `<romid>`, so a child without `internalidaddress` is matched at address 0, though the loaded ROM inherits its base's address. Once a `<rom>` matches, the search stops even if loading it fails. "Load anyway" takes the file's first `<rom>`, usually the abstract base (`xml/DOMRomUnmarshaller.java:81-93,112-131`; `editor/ecu/OpenImageWorker.java:246-250,302-305`). | Match on the resolved ROM ID (after inheritance) through the ROM-ID index. A failed load is reported and the next candidate considered; a forced load names its definition explicitly (EC-03). | LEG-DEF-03 (US-RR-39-10) |
| MF-57 | P2 | Editor | An axis's user level is its own `userlevel` (default 0), and the edit and save checks use the cell's own table. So the axis cells of a level-4 table can be edited at level 1 (when only axis cells are selected) and are always saved (`xml/TableScaleUnmarshaller.java:189-190`; `maps/DataCell.java:236,486-487`; `maps/Table3D.java:447-457`). | Axes take their table's user level. | LEG-DEF-23 (US-RR-39-9) |
| MF-58 | P1 | Definitions | Static axis data is lost. `<data>` fills only the axis's current cell count. When the axis's own `sizex`/`sizey` differs from the parent's, the parent resizes it with a fresh empty array, discarding every static cell while the axis stays marked static, so its cells stay null (`maps/Table.java:105-115,150,274-276`; `xml/TableScaleUnmarshaller.java:235-237`). | A static axis whose `<data>` count disagrees with its table's size is an Error Diagnostic; static values are never discarded. | LEG-DEF-15 (US-RR-39-8) |
| MF-59 | P2 | Definitions | The current scaling depends on element order: a `Default`-category scaling added after the preferred (`defaultscale`) category takes over (`maps/Table.java:230-239`). | Scaling selection is a function of the resolved set: the preferred category, else `Default`, regardless of order. | LEG-DEF-21 (US-RR-39-9) |
| MF-60 | P1 | Definitions | Attribute values are parsed inconsistently. Table `endian` maps anything unrecognised to little, `<memmodel endian>` maps anything but `little` to big, so a typo flips byte order silently, and differently in each place. `storagetype` prefixes are case-sensitive (`UINT8` throws) while signedness is lower-cased (`xml/RomAttributeParser.java:36-46,52-81`; `xml/TableScaleUnmarshaller.java:167`). | One parser per attribute type, case-insensitive where legacy files vary. An unrecognised value is an Error Diagnostic, never a default byte order. | LEG-DEF-11, LEG-DEF-12 (US-RR-39-8) |
| MF-61 | P0 | Encoding | `uint32` raw values of `2^31` and above cannot be written. Saving casts the raw `double` to `int`, which saturates to `7FFFFFFF` (also 4-byte Bosch-subtract; reachable through undo, interpolation and the Bosch cascade). Editing goes through `(int) Math.round`, which wraps: 3 000 000 000 becomes −1 294 967 296 and is clamped to 0 (`maps/DataCell.java:261,447-449,530`; `maps/Table.java:635`). | Raw values are carried at full width from conversion to bytes; out of range is rejected, never wrapped or clamped (EC-08). | LEG-ENC-12 (US-RR-44-11) |
| MF-62 | P0 | Encoding | Masked (bitfield) writes corrupt neighbouring bits. The per-byte mask is cut most-significant byte first while the value bytes are in table order, so a multi-byte little-endian masked cell clears the wrong byte and ORs the field into an uncleared one. The shifted value is ORed in whole, so a signed table's field, a split mask, or any path past the clamp spills into neighbouring bits. Reading never sign-extends a field (`maps/DataCell.java:105-139,264-294`; `xml/RomAttributeParser.java:124-131,193-224`). | A bitfield write changes only the bits of its mask, in the cell's byte order. A value wider than the field is rejected; signed fields sign-extend (EC-06). | LEG-ENC-06 (US-RR-44-11); LEG-ENC-07 (`fixtures/kat/bitfield-u16-le.toml`) |
| MF-63 | P0 | Editor | Bitwise-switch undo flips other flags. A bit cell's range and `originalValue` come from a first read of the whole storage word, before its bit mask is set, so undo writes the whole byte shifted into the bit's position (byte `2A`, bit 3 becomes `72`) (`maps/DataCell.java:77-84,368-371,547-549`; `maps/TableBitwiseSwitch.java:49-51`). | Undo restores the Patch's original bytes (C10); a bit cell's value is only its bit. | LEG-ENC-08 (US-RR-44-11) |
| MF-64 | P0 | Encoding | `MOVI20` does not round-trip. Read takes the `<< 8` from the instruction's `s` bit, write sets the `s` bit from the storage type, so a mismatched table changes the instruction's type on save. `movi20s` drops the low 8 bits of a written value, and both drop bits above 20 silently. A little-endian table reads garbage; the 3-byte stride cannot address consecutive 4-byte instructions; a cell at image offset 0 throws (`xml/RomAttributeParser.java:137-158,181-236`; `maps/DataCell.java:169-175,271-275`). | The MOVI20 codec keeps the opcode byte and nibble, rejects values outside its range and any byte order but big, and is addressed per instruction (EC-06). | LEG-ENC-02 (`fixtures/kat/u20-movi20.toml`, US-RR-44-11) |
| MF-65 | P1 | Encoding | Bosch-subtract values are clamped to the storage range, though `2^(8W)` and negative values are valid in that layout (`maps/DataCell.java:105-139,193-234`). | The Bosch-subtract range derives from the layout; an out-of-range edit is rejected, not clamped (EC-06, EC-08). | LEG-ENC-09 (US-RR-40-11) |
| MF-66 | P1 | Conversion | Scaling formulas are not validated. A formula that does not parse, or names anything but `x`, evaluates to NaN in every cell, because `JEPUtil` never checks `hasError()`. `Scale.validate` compares `to_byte(expression(5))` with 5 only, and a NaN difference passes. `setRealValue` treats `X` (any case) as identity while JEP's variable is lower-case `x`, so `expression="X"` displays NaN but an edit writes the typed number (`util/JEPUtil.java:46-60`; `maps/Scale.java:63-77`; `maps/DataCell.java:436-439`). | Formulas are parsed once at load (MF-25); a parse error or unknown name is a load Error. `to_byte` is checked against `expression` over the raw range, and NaN fails. | LEG-ENC-13, LEG-ENC-16 (US-RR-44-11) |
| MF-67 | P1 | Logger defs | When the settings' protocol or transport is not in the logger file, legacy switches to the first entry of a `HashMap` (undefined order) and re-parses, silently changing the user's protocol. A file whose protocols have no transports throws instead, so the pinned v290 files cannot load (`logger/ecu/definition/EcuDataLoaderImpl.java:112-118,137-163`). | A missing protocol or transport is a Diagnostic and the user chooses; a protocol without transports loads with its default module. | LEG-LOG-02 (US-RR-30-10) |
| MF-68 | P0 | Logger defs | `<ecuparam>` per-ECU data is mishandled. ECU ID lists are split on `,` without trimming, so an ID after `", "` never matches and that ECU silently loses the parameter (v290 has one). The conversions take the `bit` of the **last** `<ecu>`, so ECUs listed earlier with another bit decode the wrong bit (four v290 MerpMod parameters) (`LoggerDefinitionHandler.java:356-365,378-383`). | ID lists are trimmed; each ECU keeps its own address, bit and conversions. | LEG-LOG-08 (US-RR-30-11) |
| MF-69 | P1 | Logger defs | Derived-expression references: only the first `[ID:` of a dependency is rewritten, so a second unit of the same dependency evaluates to NaN (then logged as 0, MF-09). A reference to a unit the dependency lacks throws during parsing, so one typo fails the whole logger definition (`logger/ecu/definition/EcuDerivedParameterConvertorImpl.java:107-126,137-141`). | References are parsed into the Expr AST (C4) with every `[ID:unit]` resolved; an unknown unit is a per-signal Diagnostic. | LEG-LOG-09 (US-RR-30-11) |
| MF-70 | P1 | Logger defs | A parameter with several `<address>` elements and `length` ≥ their count keeps only the first address (expanded by `length`) and drops the rest silently (`logger/ecu/definition/EcuAddressImpl.java:76-95`). | Conflicting `<address>` lists and `length` are a Diagnostic. | LEG-LOG-03 (US-RR-30-10) |
| MF-71 | P1 | Logger defs | The `target` attribute (ECU, TCU, both) is parsed and never used, so TCU parameters are offered and polled against the ECU, and vice versa (`LoggerDefinitionHandler.java:234,281,291`). | `target` maps each parameter to its module; only parameters of a connected module are offered. | LEG-LOG-12 (US-RR-30-10) |
| MF-72 | P2 | Dyno | The only legacy CSV reader (dyno import) cannot read legacy's own logs in absolute-time mode or with a mid-file header, catches only `IOException`, and picks columns by header substring with the last match winning, so a parameter named `…Times…` or `…Throttle…` can take over the time or throttle column (`logger/ecu/ui/tab/dyno/DynoControlPanel.java:1009-1094`). | Log import reads the port's CSV and legacy CSVs (EC-28) and maps columns by Role (C13, MF-31). | LEG-CSV-12 (US-RR-58-8) |
| MF-73 | P1 | I/O | Text files use the platform default charset. Logs are written with no declared charset, so `°`, `µ` and non-ASCII names differ between Windows and Linux. `settings.xml` declares ISO-8859-1 but is written with `FileWriter` in the platform charset, so on a UTF-8 platform each save double-encodes non-ASCII paths (`é` → `Ã©` → …). The ROM-ID ASCII comparison decodes image bytes in the platform charset (`logger/ecu/ui/handler/file/FileLoggerImpl.java:111`; `xml/DOMSettingsBuilder.java:72,78`; `util/SettingsManager.java:78,88-92`; `maps/RomID.java:83-87`). | Every text file the port writes is UTF-8; importers detect legacy encodings; ROM-ID ASCII is compared as bytes. | LEG-CSV-05 (US-RR-57-9); LEG-SET-04 (US-RR-59-10); LEG-DEF-03 (US-RR-39-10) |
| MF-74 | P1 | Logger | Log files lose data. Names have one-second resolution and are opened with truncation, so two logs started in the same second (a defogger toggle) overwrite each other; the user's "Logfile Text" goes into the path unsanitised. Rows sit in an unflushed `BufferedOutputStream` until the file is closed, so a crash, power loss or unplugged laptop loses the tail (`logger/ecu/ui/handler/file/FileLoggerImpl.java:44,63,77-88,132-145`; `logger/ecu/EcuLogger.java:1406-1414`). | Unique log names that never truncate an existing file; sanitised user text; rows flushed at a bounded interval, so a crash loses at most that interval. | LEG-CSV-08, LEG-CSV-09 (US-RR-57-10) |
| MF-75 | P2 | Logger | Row timestamps are wall-clock time taken after the whole poll cycle, and absolute mode prints time of day without a date, so relative times go backwards on a clock step and absolute times wrap at midnight (`logger/ecu/comms/query/ResponseImpl.java:32-34`; `logger/ecu/ui/handler/file/FileLoggerImpl.java:45,121-130`). | Samples carry a monotonic timestamp taken at receipt; the wall-clock start, with its date, is recorded once (EC-26). | LEG-CSV-11 (US-RR-57-10) |
| MF-76 | P1 | Settings | Any malformed `settings.xml` value (a non-numeric integer, an empty `switchid`, a `locale` without `_`) or a malformed file throws out of `load`, and RomRaider does not start; only the icon scales fall back (`util/SettingsManager.java:106-108`; `xml/DOMHelper.java:53-59`; `Settings.java:648-651,886-891`). | A bad value is a Warning and takes its default; a malformed file is reported and kept, and the app starts with defaults. | LEG-SET-05 (US-RR-59-10) |
| MF-77 | P1 | Settings | Settings change across load and save. Phidget sensor scalings are written only inside `<plugins>`, which is written only when a plugin port is set, so without one they are dropped on save. Field and reader defaults disagree for fifteen settings, so a missing attribute behaves differently from a missing element (the window-size fallbacks are swapped). `image_dir` falls back to the file name `ecu_defs.xml`, and an empty log path puts logs in the filesystem root (`xml/DOMSettingsBuilder.java:398-426`; `xml/DOMSettingsUnmarshaller.java:85-87,113,135,278-285`; `logger/ecu/ui/handler/file/FileLoggerImpl.java:133-136`). | One default per setting, in the typed schema; save writes every setting; an empty path falls back to the documented default directory. | LEG-SET-03, LEG-SET-07 (US-RR-59-10) |
| MF-78 | P1 | Comms | `PollingStateImpl` keeps all its fields `static` and its constructor resets them, so every `new PollingStateImpl()` (init, write, reset, DTC read, learning tables, global adjust, RamTune) resets the logger's poll state, and the `A8`/`A0` mode byte depends on whatever touched it last (`logger/ecu/comms/manager/PollingStateImpl.java:23-37`; `iso9141/SSMProtocol.java:62,214-215`). Sibling of MF-29. | Poll state belongs to the session; codecs are stateless (as MF-29). | Sim test: a DTC read during a continuous poll leaves the poll's mode byte unchanged |
| MF-79 | P0 | Comms | SSM length fields overflow silently. `LL` is `Integer.byteValue()` with no cap, so an `A8` of 85 or more addresses, or a `B0` of more than 251 data bytes, sends a wrong `LL`. The `A0` count byte `asByte(count - 1)` wraps above 256, and RamTune never validates its block size. The continuous-mode check compares the signed `frame[3]` with an int, so any reply with `LL ≥ 80` always fails (`iso9141/SSMProtocol.java:98,201-211`; `SerialConnectionManager.java:93`; `ramtune/test/RamTuneTestApp.java:242-244`). | Request encoders reject a length or count their field cannot carry; length fields compare unsigned. | LEG-SSM-06 (US-RR-47-12) |
| MF-80 | P0 | Comms | The SSM write acknowledgement skips `validateResponse`: a corrupt frame (bad checksum, wrong IDs or length) whose bytes 4 and 5 happen to match is taken as a successful write. A reply shorter than 6 bytes throws; RamTune's `B0` reply is not compared with the data (`iso9141/SSMProtocol.java:152-163`; `ramtune/test/RamTuneTestApp.java:307-312`). | A write is acknowledged only by a fully validated reply whose code and data match the request. | LEG-SSM-16 (`fixtures/public/ssm/errors.toml`, US-RR-36-10) |
| MF-81 | P0 | Logger | SSM continuous mode reads one frame per cycle and never drains the backlog, so displayed values can lag real time without bound, with no timestamp (`SerialConnectionManager.java:76-87`). MF-08 and MF-26 cover other staleness paths. | The continuous reader drains to the newest complete frame and timestamps each sample at receipt; a lagging stream is a link-health event. | Sim test: a stream faster than the poll loop shows the newest frame (with LEG-SSM-09, US-RR-36-10) |
| MF-82 | P2 | Comms | Unknown-length exchanges (init, every write, reset, all of RamTune) end only after `connect_timeout` (2000 ms) of silence, with no end-of-frame detection from `LL`: slow, and any late byte extends them (`SerialConnectionManager.java:107-129`). | A frame ends at the length its header declares; silence only ends a read as a timeout (C9). | LEG-SSM-07 (US-RR-36-10) |
| MF-83 | P1 | Logger | DTC range by init length: `if (len < 104) … else if (len < 56)` makes the second branch unreachable, so 40-byte-init ECUs read the `D488` range rather than `D256` (`logger/ecu/comms/readcodes/ReadCodesManagerImpl.java:69-74`). | The DTC range comes from the ECU's capability data, and the 40-byte-init case is tested. | Test: a 40-byte init selects the `D256` range |
| MF-84 | P1 | Comms | SSM reply validation accepts any of the five response codes on a read, whatever command was sent, so a reply to another request is decoded as data (`iso9141/SSMResponseProcessor.java:70`). | A reply's code must be its request's command + `40`; anything else is a typed decode error. | LEG-SSM-05 (US-RR-47-12) |
| MF-85 | P1 | Comms | `sendAddressWrites` silently skips any query whose address is not exactly 3 bytes, so a multi-byte parameter is never written and the caller is not told (`SSMLoggerConnection.java:163-164`). | A write the codec cannot encode is an error to the caller; multi-address writes are encoded or refused, never skipped. | Sim test: a write to a 2-address parameter writes both bytes or fails (with LEG-SSM-16, US-RR-36-10) |
| MF-86 | P1 | Editor | Cell edit text is parsed with `NumberFormat.parse` in the default locale, which accepts a valid prefix (`12abc` is 12) and skips grouping separators (`1,5` is 15 in an English locale); unparsable text is ignored without a message (`util/NumberUtil.java:31-35,55-57`; `maps/DataCell.java:455-457`). | Edit input is parsed whole, with one documented decimal rule and no grouping; anything else is refused with a message. | Editor T0 test: `12abc` and `1,5` (English locale) are refused (US-RR-61) |
| MF-87 | P1 | Logger defs | One bad number or missing required value anywhere in the selected protocol section (`baud`, `length`, a switch `bit` outside 0–7, a non-numeric `ecubyteindex`/`ecubit`, a module's hex address, an empty `units`/`expr`/`format`) throws out of the SAX handler, and the whole logger definition fails to load (`LoggerDefinitionHandler.java:191-202,238-240,282,464,468`; `EcuSwitchConvertorImpl.java:33`). | The bad element gets a Diagnostic and the rest of the file loads. | LEG-LOG-14 (US-RR-30-10) |

## Edge Cases (must have tests)

Each needs at least one test (tier in brackets) before its area's milestone closes.

**ROM, definitions and encoding**
- EC-01 Tables overlapping the same bytes, including different storage widths and endianness [T1, T2]
- EC-02 Table address range past the end of the ROM, or ROM size ≠ definition size → Diagnostic, table disabled, never a panic [T0, T3]
- EC-03 Two definitions match the same ROM ID (ambiguous) → refuse to auto-pick and ask the user [T0]
- EC-04 Inheritance diamond, cycle, omit-then-override, cross-file base [T1]
- EC-05 Axis tables shared by several tables; editing an axis updates every dependent view [T0]
- EC-06 MOVI20 (20-bit), Bosch-subtract layout, bitfields spanning byte boundaries, signed bitfields [T1, T2]
- EC-07 NaN, ±Inf and denormal floats in the ROM: displayed as such, editing is rejected unless explicitly allowed [T0]
- EC-08 Non-monotonic or non-invertible scalings; division by zero at some raw value; `to_raw` producing out-of-range values → edit rejected, never wrapped or clamped [T1, T6]
- EC-09 Definition files in ISO-8859-1, UTF-8 with BOM, CRLF; hostile XML (entity expansion, huge attributes) [T3]
- EC-10 Large images (≥4 MB) and definitions with 5k+ tables: load, resolve and decode within budget [perf] (resolve and ROM-ID lookup: `perf-budget` CI gate, US-RR-34-4)

**Checksums and saving**
- EC-11 Checksum ranges that overlap the checksum storage itself [T1, T2]
- EC-12 Checksums that depend on other checksums (Nissan ALT2 order, E38 two-pass) [T2]
- EC-13 Save interrupted (crash or power loss mid-write) → original intact, `.bak` present [T6]
- EC-14 Save to a read-only location, a path with spaces, unicode or a network share [T0]

**Comms**
- EC-15 ECU stops responding mid-session → recovery Rules run, then a clean disconnect with no stale values [T5]
- EC-16 K-line echo absent, partial, or delayed past the response start [T5]
- EC-17 Response split across reads; garbage before the header; two responses in one read [T1, T5]
- EC-18 USB adapter unplugged mid-transaction → Link error, no hang, no panic [T5, T9]
- EC-19 J2534 DLL only available as 32-bit on a 64-bit host → clear Diagnostic and the documented 32-bit build path [T0, T9]
- EC-19a Linux: port access denied (`EACCES`) → `doctor` explains and offers the udev rule install; no generic "failed to open port" [T0]
- EC-19b Linux: ModemManager probing the adapter, or `brltty` claiming a CH340/FTDI cable → detected and explained with the fix [T0, T9]
- EC-19c Linux: adapter re-enumerates to a different `/dev/ttyUSBn` after replugging → reconnect by stable ID (`/dev/serial/by-id`), not by index [T5, T9]
- EC-20 Unknown ECU ID → only the base profile applies, and the user is warned [T4]
- EC-21 More parameters requested than the ECU's per-request limit → engine splits batches (Rule `max_items`) [T5]
- EC-22 Baud change mid-session (DS2, NCS) [T5]
- EC-23 Quirk rules: per-op delay, ordering of layered overrides, disable-one / disable-all [T1, T4]

**Live data and logs**
- EC-24 Wideband device stale or unplugged; values go `None` after the TTL [T5]
- EC-25 Derived signal depending on a missing or stale input → `None`, not 0 [T1]
- EC-26 Wall-clock jumps (NTP, DST) during logging: monotonic timeline, wall time recorded separately [T0]
- EC-27 Parameter set changes during a log → new CSV file, playback handles the rotation [T0]
- EC-28 Importing Java-era CSVs with locale decimals and Cobb/AEM/OP2 headers [T2]

---
*Last reviewed: 2026-10-22*
