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
| `fixtures/kat` | Hand-authored known-answer fixtures (TOML), each expected value with a stated source |
| `fixtures/public` | Committed synthetic ROMs, definitions, traces and golden outputs (no proprietary firmware) |
| `fixtures/private` | Git-ignored. Real ROMs and traces, path set by `RR_FIXTURES_DIR` |
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
cargo nextest run                     # all tests (public fixtures)
RR_FIXTURES_DIR=~/rr-fixtures cargo nextest run --features private-fixtures
cargo insta review                    # review snapshot changes
cargo llvm-cov nextest                # coverage
cargo mutants -p rr-core -p rr-rom    # mutation testing (safety crates)
cargo +nightly fuzz run <target>      # fuzz a reader/parser
cargo xtask gen-synthetic             # regenerate synthetic ROMs from the reference encoder
```

**Env vars** (names only):
- `RR_LOG` sets the tracing filter.
- `RR_FIXTURES_DIR` is the private fixture corpus.
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
| MF-01 | P0 | Checksum | E38 PCM `update` ignores the `calculated` flag, so it writes zeroed sums when segment validation fails. Its correction count is also doubled by the two-pass loop (`maps/checksum/ChecksumE38PCM.java` ~59 vs ~77). | Only write sums for validated segments. If any segment fails, return an error and write nothing. The two-pass dependency becomes an explicit ordered spec. | T2 fixture + T6 "invalid segment → no patch" + T7 |
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
| MF-18 | P1 | Definitions | Table `base` uses a Java-serialization deep copy, which probably clones the whole ROM object graph (`xml/TableScaleUnmarshaller.java:90`). | Shared base via `Arc`, override on resolve. | Perf test: 1000-table definition resolves < 200 ms |
| MF-19 | P1 | Checksum | The legacy Subaru checksum is triggered by a table *named* "Checksum Fix", with edit-stamp offsets +204/+207 hard-coded (`maps/Rom.java:337,455`, `maps/DataCell.java:211,482`). | ChecksumSpec in the definition family profile. | T2 Subaru fixture |
| MF-20 | P1 | Import | VDF import drops any scale factor between 0.99 and 1.01 (`xml/ConversionLayer/VDFConversionLayer.java:211`). | Keep exact factors. | T2 VDF fixture |
| MF-21 | P1 | Import | XDF problems: equations fixed up by string replace (X→x, ","→"."); REGION filesize created but never appended; float XDFs ignored; `lsbfirst` default and 0x02 flag use opposite polarity (`xml/ConversionLayer/XDFConversionLayer.java:275,278,507,514`). | Parse into the Expr AST; support float; one polarity rule; full region. | T2 + T3 fuzz on XDF |
| MF-22 | P1 | Editor | ROM compare is O(n²) with inconsistent equality (1D ignores the name) and compares raw values regardless of scaling (`swing/CompareImagesForm`). | Diff = Patches (C10), compared both raw and real. | Diff property tests |
| MF-23 | P1 | Editor | `Rom.populateTables` removes from the map while iterating, and uses NPE as control flow (`maps/Rom.java:320-324`). | Diagnostics, no exceptions as control flow. | Covered by resolver tests |
| MF-24 | P1 | Logger defs | Derived params with missing dependencies silently vanish; derived params can't use external sensors (`logger/ecu/definition/xml/LoggerDefinitionHandler.java:336`). | Derived graph over Signals, topologically sorted; a missing dependency gives a Diagnostic. | Derived-graph tests incl. an external dependency |
| MF-25 | P1 | Expr | Two JEP dialects: the map overload lacks the standard functions and `BitWise`. Global `synchronized` cache of 32 entries; derived params run `String.replace` per sample (`util/JEPUtil.java:47,63`; `EcuDerivedParameterConvertorImpl.java:63`). | One parser, one dialect, parsed once at load. | T1 + T2 conversion fixtures |
| MF-26 | P1 | Devices | External values have no timestamp or staleness check (`logger/external/core/ExternalDataConvertorImpl.java:48`). | Timestamped Signals with TTL; stale → `None`. | T5 device-unplug fault |
| MF-27 | P1 | Comms | Read timeout counts `sleep(1)` iterations, not wall-clock time. With Windows timer granularity the real timeout can be ~15× longer (`io/serial/connection/SerialConnectionManager.java:76-79`). | Deadlines from Clock (C9). | T4 VirtualClock timeout test |
| MF-28 | P1 | Comms | Magic `sleep(55L)` after the NCS ECU-ID request (it is ISO 14230 P3min), duplicated alongside J2534 `P3_MIN` config (`logger/ecu/comms/io/connection/NCSLoggerConnection.java:111`). | Timing from LinkProfile; every legacy sleep mapped to a named timing or quirk Rule. | T4 timing assertions |
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
| MF-40 | P2 | Editor | A bad scaling raises a popup from model code (`maps/Table.java:558`). Every ROM open re-parses every definition file. | Diagnostics; cached ROM-ID index. | Diagnostic test; perf test |
| MF-41 | P2 | Logger | Dash warning thresholds are not saved in the profile (TODO, `logger/ecu/EcuLogger.java:208`). | Part of the profile schema. | Profile round-trip |
| MF-42 | P2 | Dyno | `KPH_2_MPH = 1.609344` is actually km per mile, so the name is inverted (also `logger/car/util/Constants`). | Units registry (C5). | Units tests |
| MF-43 | P2 | Startup | A missing i18n bundle gives null and a crash (`ECUExec.java:65`). | Strings embedded at compile time. | — |
| MF-44 | P2 | Tests | `XDFConversionLayerTest` points at `C:\Users\User\Downloads\…`, so it tests nothing. | Fixtures in-repo or in the private corpus. | — |

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
- EC-10 Large images (≥4 MB) and definitions with 5k+ tables: load, resolve and decode within budget [perf]

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
*Last reviewed: 2026-10-19*
