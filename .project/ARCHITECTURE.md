# RomRaider-RS — Architecture

## Overview

This is a single-process desktop application: ROM editor and logger in one app. It is built as a Cargo workspace of small crates around one rule: **as few distinct concepts as possible, each reused everywhere it fits.**

The Java codebase solves the same problem four or five different ways. Examples:
- ROM `base` inheritance, logger definition filtering, comms timing and wideband plugins are each layered configuration, but each is implemented differently.
- ROM table cells, logger parameters and wideband fields are each "bytes at an offset → number via a formula", again in three separate implementations.

Here, each idea exists exactly once.

The Java source lives in `legacy/` as **read-only reference material**: we read it to understand existing behaviour and file formats. **No Java is built, run or written as part of this project** (D-024). The Cargo workspace sits at the repo root, with crates in `crates/` and fixtures in `fixtures/`.

## Concept Catalogue

This is the heart of the architecture. **Before adding a struct, enum or trait that crosses a crate boundary, check this table.** If a concept fits, use it. If none fits, add a DECISIONS.md entry and a row here.

| # | Concept | Lives in | What it is | Used by (replaces in Java) |
|---|---------|----------|------------|----------------------------|
| C1 | **Layered\<T\> + Resolver** | `rr-core::layer` | Ordered layers of partial docs. `extends`/`base`, override, append, `omit`. Cycle detection, provenance per field, diagnostics. One `resolve()` for everything. | ROM definition inheritance (recursive `base` + serialization deep-copy + `StackOverflowError` cycle check); table `base`; comms link profiles + quirks (protocol → adapter → ECU family → ECU ID); checksum specs; device profiles; logger parameter sets (filtered at parse time today); settings (defaults → user file → CLI) |
| C2 | **Pattern** | `rr-core::pattern` | Match on bytes with mask/wildcards at an offset, on a named op, on an ID prefix, or on header names. Compiled once, matched cheaply. | ROM identification (address + bytes); ECU-ID → profile selection; quirk `when` clauses; device frame sync headers; response validation; CSV log-format detection (Cobb/AEM/OP2 header guessing in DynoControlPanel) |
| C3 | **Encoding** | `rr-core::encoding` | `{ storage: U8..U64/I8..I64/F32/F64/U20(MOVI20)/U24…, endian, bitfield: Option<mask/shift>, layout: Default/BoschSubtract }`. One codec, `decode(&[u8]) -> Raw`, `encode(Raw) -> bytes`. | ROM table cells (int `storageType` with magic 20/28/99, static endian, int24/int64 → 0); logger ECU parameter bytes; wideband frame fields; protocol header fields |
| C4 | **Expr / Conversion** | `rr-core::expr` | Formula parsed once into a typed AST (JEP-compatible dialect + native bit operators + `[param:unit]` refs). `Conversion = Linear{m,b} \| Expr{to_real, to_raw?} \| Lut`, with unit and display format. Exact inverse for linear forms; bounded numeric solve otherwise; non-invertible means an error. | ROM scalings (JEP strings evaluated per cell per repaint behind a global lock); logger conversions; derived parameters (`String.replace` per sample); device field conversions; checksum-free arithmetic in dyno/MAF maths |
| C5 | **Units** | `rr-core::units` | Unit registry: display ranges, conversions between units, gauge defaults. | `ConverterMaxMinDefaults` keyed by unit string; Imperial/Metric string branches in dyno; AFR/fuel-type enums |
| C6 | **Framing** | `rr-core::framing` | How a byte stream splits into frames: `LengthByte{offset,overhead}`, `KwpHeader`, `IdleGap`, `IsoTp`, `Line{delim}`, `Sync{header: Pattern, len, checksum}`, `Fixed(n)`. | ECU response framing (known length / idle-gap loops); every wideband plugin's hand-written parser loop |
| C7 | **Link** | `rr-link` | The one I/O trait: `transact(op, req, expect) -> Frame` and `stream() -> frames`. Impls: Serial, J2534, ELM327, OpenPort2 (native USB, D-023), **Replay**, **Sim**. Links push timing to adapters that support it and enforce it in software otherwise. | jSerialComm wrappers, J2534 connections ×3, ELM connection, 13 device connection classes, ~1.1k LOC of mock connections |
| C8 | **Rule (when → do)** | `rr-core::rule` | `when: Pattern + phase + prev-op`, `do: [Action]` from a fixed action set (delay before/after, timeout, retry, max_items, drop_leading, ignore_checksum, reinit_after, named hook). Each rule has an `id`, a `note` and a `source`. Flattened per op at resolve time. | Comms quirks (`sleep(55L)` etc.); **simulator fault injection (the same rules, applied on the ECU side)**; legacy float-endian behaviour as a definition-family rule |
| C9 | **Clock** | `rr-core::clock` | Injected monotonic time. `RealClock` in production, `VirtualClock` in tests, so timing tests are deterministic and instant. | `currentTimeMillis()`, `sleep()` polling loops, timeouts counted in sleep iterations |
| C10 | **Patch / Journal** | `rr-rom::journal` | Every mutation of a byte image is a `Patch { range, old, new, origin }` in a journal. Undo, redo, "modified" markers, diff between two ROMs, checksum fix-ups, and "apply changes from another ROM" are all patches. | `DataCell` original/compare copies, revert-only undo, `byteCellMapping` overlap sync, O(n²) image compare |
| C11 | **Table (N-D grid)** | `rr-rom::table` | `Table { axes: Vec<Axis>, cells: Grid<CellRef>, kind }`. A cell is a view onto bytes (Encoding + Conversion), never a copy. Each operation is written once over a `Selection`. | Table1D/2D/3D/Switch/Bitwise (~950 LOC of triplicated overrides); logger learning-table results ×3 panels; MAF/injector collector bins; live-data overlay |
| C12 | **Signal / SampleFrame** | `rr-live` | Every live value is a `Signal { id, unit, role? }`. A `SampleFrame { t, values: Vec<Option<f64>> }` is indexed densely and broadcast over a bounded channel. Producers: comms engine, devices, **CSV playback**. Consumers: UI, CSV writer, collectors, table overlay. | `Response` objects scanned linearly by every handler; one `invokeLater` per parameter per sample; missing value → 0.0; untimestamped device values; separate playback stub |
| C13 | **Role** | `rr-defs` | Semantic name for a signal (`engine_speed`, `maf_volts`, `cl_ol`…), mapped per protocol in definitions. | Hard-coded Subaru IDs `"P8"`, `"E33"` in the MAF/Injector/Dyno tabs |
| C14 | **Collector** | `rr-live::collect` | `trait Collector { fn filter(&FilterSet) ; fn accept(&SampleFrame) }` plus a generic `FilterSet<RangeFilter>`. Output is a Table (C11). | MafUpdateHandler / InjectorUpdateHandler duplicate 8-step validation chains; dyno run capture |
| C15 | **Diagnostic** | `rr-core::diag` | `{ severity, provenance, message, code }`, collected rather than thrown. Loading always returns `(T, Vec<Diagnostic>)`. | JOptionPane popups from model code; swallowed exceptions; derived params silently dropped |
| C16 | **Trace** | `rr-link::trace` | Timestamped record of every frame and quirk applied on a Link. **The trace format is the fixture format.** A trace recorded on a real car becomes a replay test. | `LOGGER.debug(asHex(...))` scattered through comms |

### Composition examples

These show the reuse in practice:

- **A wideband device** is a profile: Link (stream mode, C7) + Framing (C6) + fields (Encoding C3 + Conversion C4) → Signals (C12). That's zero device-specific code for about 8 of the 13 Java plugins. Odd devices (PLX, Innovate, Ecotrons) implement one small `FrameParser` hook.
- **An ECU logger parameter** is Encoding (C3) + Conversion (C4) + address. That is the same shape as a ROM table cell, so the logger and editor share it.
- **The ECU simulator** is a protocol codec running in reverse. It serves a ROM/RAM image (C10's byte image) over a Sim Link (C7), with faults injected by Rules (C8) and timing on a VirtualClock (C9).
- **ROM identification and quirk selection** are the same Pattern (C2) match. The first picks a definition; the second picks a profile layer.
- **Compare two ROMs** = a diff into Patches (C10). **Fix checksums** = compute Patches. **Write to the ECU** (future) = apply a Patch set to blocks.

## Crate Map

| Crate | Depends on | Responsibility | I/O? |
|-------|-----------|----------------|------|
| `rr-core` | — | C1–C6, C8, C9, C15. Pure types and algorithms. | None |
| `rr-defs` | core | Format readers: RomRaider ECU XML, logger XML, cars XML, XDF, (later ECUFlash XML), TOML profiles → `Layered` docs. Resolves into frozen typed `Definition`, `LinkProfile`, `DeviceProfile`, `ChecksumSpec`, `ParamSet`. Cached ROM-ID index. | File read only |
| `rr-rom` | core, defs | Byte image, Patch journal (C10), Table (C11), checksum engine + small named-algorithm registry, clipboard codec, diff. | None (caller supplies bytes) |
| `rr-link` | core | Link trait (C7), Serial / J2534 (libloading) / ELM327 / OpenPort2 (nusb) / Replay / Sim impls, Trace (C16). Port discovery by stable ID (`/dev/serial/by-id` on Linux). | Yes |
| `rr-comms` | core, defs, link | Transaction engine (applies timing + Rules), protocol codecs (SSM, KWP2000/NCS, OBD, DS2) as small trait impls, ECU init/identify flow, memory-write gating. | Via Link |
| `rr-live` | core, defs, comms, link | Signals and SampleFrames (C12), device runner, derived-signal graph (topo-sorted), Collectors (C14), CSV writer and CSV playback producer. | Via Link / files |
| `rr-testkit` | all | Fixture loaders, golden-snapshot helpers (`insta`), proptest strategies for C3/C4/C1, ECU simulator wiring, VirtualClock harness. dev-dependency only. | Test only |
| `rr-cli` | all but app | Headless `decode`, `verify`, `diff`, `edit`, `fix-checksums`, `log`, `replay`, `import-settings`. Also the main integration-test surface. | Yes |
| `rr-app` | all but cli | egui/eframe desktop app: editor + logger in one process, single-instance via local socket/named pipe. | Yes |

**Dependency rule:** `rr-core` depends on nothing in the workspace. Only `rr-link`, `rr-cli` and `rr-app` touch the OS. Everything else is testable without hardware or a filesystem.

## Contracts

These are the interfaces between pieces:

- **Definition files (input, compatibility contract).** Existing RomRaider ECU-definition XML and logger-definition XML must load unchanged. The fixture corpus is the contract. XDF and ECUFlash are import-only.
- **Profile files (new, our contract).** TOML for link profiles, quirk rules, device profiles, checksum specs and settings. Each file declares `schema = N`, and old schemas get migration functions with tests.
- **Trace format (`.rrtrace`).** Versioned, line-delimited JSON. It holds timestamped tx/rx frames, link events and quirks applied. It is written by Links and read by Replay. The same format serves recorded fixtures and debugging dumps.
- **Fixture format.** Read by `rr-testkit`, versioned with `schema = N`. Two kinds:
  - Hand-authored, reviewed TOML known-answer files: a definition snippet + bytes + expected raw/real values, or checksum before/after.
  - Synthetic ROMs with manifests, generated by the independent reference encoder.
  - Every expected value states its source: hand-calculated, standard document, stock ROM, car trace or cross-source agreement.
- **CSV logs.** Schema fixed at file start, raw numbers, `.` decimal separator, RFC 4180 quoting. Header-compatible with Java RomRaider logs for import. A schema change starts a new file.
- **Settings.** TOML under `directories::ProjectDirs`. One-time importer for the Java `settings.xml`.

## Cross-cutting Standards

### Errors
- `thiserror` enums per crate. `anyhow` only in `rr-cli` and `rr-app`.
- Loaders return Diagnostics (C15) for recoverable problems.
- **Library code never panics on input, never prints, never shows UI.**
- `unwrap()`/`expect()` are banned outside tests and provably-infallible spots, which need a comment. Enforced with clippy `unwrap_used`.

### Logging
- `tracing` everywhere, filtered by `RR_LOG`.
- Comms produce Traces (C16) rather than ad-hoc hex debug lines.

### Threading
- No async runtime in the core.
- Each live Link runs on a dedicated OS thread. Serial and J2534 DLL calls are blocking anyway.
- Threads communicate over bounded `crossbeam-channel`s and stop via cancellation tokens. No shared mutable state, no `static mut`, no global singletons.
- The UI thread only reads the latest frame and sends commands.

### Time
- All timing goes through Clock (C9). `std::thread::sleep` and `Instant::now()` are banned outside `RealClock` (clippy `disallowed_methods`).

### Safety gates
- Every operation that writes to an ECU (memory write, global adjust, learning reset, ECU reset) or overwrites a ROM file requires a capability token. A token is only obtainable through an explicit user confirmation path. Tests assert that no code path writes without one.
- Saving a ROM:
  1. Recompute all checksums.
  2. Verify them.
  3. Write atomically (temp file + rename).
  4. Keep a `.bak` of the previous file.
  5. If verification fails, refuse unless explicitly overridden; the override is recorded in the file's sidecar journal.

### Testing

The full strategy is in PROJECT.md. Every crate has unit and property tests. Every reader and parser is fuzzed. `rr-core`, `rr-rom` and `rr-comms` are coverage- and mutation-gated in CI.

### Style
- `rustfmt` and `clippy -D warnings` in CI.
- Pinned toolchain via `rust-toolchain.toml`. MSRV tracks stable minus 2.

---
*Last reviewed: 2026-10-19*
