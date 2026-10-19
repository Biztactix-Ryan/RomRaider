# RomRaider-RS — Vision

## Problem

RomRaider is the go-to open-source ROM editor and data logger for (mostly Subaru, also Nissan/BMW/OBD) ECUs. It works, but it is ~75k lines of Java 6-era code:

- A Swing UI, about 21 MB of vendored jars and native libs, and a JVM to install.
- No consistent model. Timing quirks are hard-coded as `sleep(55L)`. There are five table classes that copy each other. Checksums are chosen by class name. There are 13 near-identical wideband plugins and a 1,000-line global settings object.
- **Ten unit tests.** Software that writes to engine computers, and can brick one, has effectively no automated verification. That is the core problem this project exists to fix.

## Who it is for

- **Tuners and enthusiasts** who edit ROM maps and log engine data on their own cars.
- **Definition authors** who add support for new ECUs and need to express quirks without writing code.
- **Contributors** who want to add a protocol, ECU or wideband device without reading 2,000-line god classes.
- **Us (Biztactix)** first: the initial target vehicle and cable set is ours. Exact make/ECU/cable is TBD (ask: Ryan).

## What success looks like

**In 6 months:**
- A headless Rust core (`rr-cli`) can:
  - open a ROM with existing RomRaider XML definitions
  - decode every table identically to the Java build, proven by differential fixtures
  - edit, save and fix checksums
  - log our target ECU over our target cable, with a recorded-trace replay suite and an ECU simulator in CI
- Every bug in the Must-Fix ledger (PROJECT.md) is fixed and covered by a test.

**In 12 months:**
- An egui desktop app with ROM editor and logger in one process, shipped as a single binary per OS with no JVM.
- Feature parity for the protocols and devices we own hardware for.
- Community definitions and quirk profiles contributed as data, not code.

## Principles

These settle arguments. In priority order:

1. **Never brick an ECU.**
   - Read-only by default.
   - Every write path is gated, verified and tested.
   - A ROM is never saved with a failing checksum unless the user explicitly overrides it.
   - Unknown means refuse: an unknown storage type, an ambiguous ROM ID or an out-of-range value is an error, never a silent 0, wrap or clamp.
2. **Tested or it doesn't exist.**
   - The Java build is the oracle: we record what it does and prove we match, or document why we deliberately differ.
   - Each layer has its test tier: unit, property, differential, fuzz, replay, simulator, hardware-in-loop.
   - Safety-critical crates must pass mutation testing.
3. **One concept, many uses.**
   - Use as few distinct structures and code types as possible. If an existing concept can express something, use it.
   - A new top-level concept needs a DECISIONS.md entry explaining why no existing one fits.
   - The concept catalogue is in ARCHITECTURE.md.
4. **Quirks are data, not code.**
   - Timing, framing, batching, retries, checksum layout, encoding oddities and device frame formats live in layered profiles, with an id, a note and a source.
   - Protocol and table code never contains special cases for a specific ECU.
5. **Boring, explicit Rust.**
   - No runtime class loading, no reflection-style plugins, no global mutable state.
   - Time is injected, errors are typed, and library code never shows dialogs.

## Roadmap

| Milestone | Goal | Exit criteria |
|-----------|------|---------------|
| **M0 Foundation & oracle** | Cargo workspace, CI, `rr-core` concept types, `rr-testkit`, Java fixture recorder, first fixture corpus | CI green on Linux and Windows. Recorder dumps decoded tables, checksums and resolved definitions from the Java build. Public synthetic fixtures committed. |
| **M1 ROM core (headless)** | Definition readers, layered resolver, Encoding, Conversion, Table, patch journal, checksums, `rr-cli decode/verify/diff/edit` | 100% differential match on the fixture corpus (or a ledger entry per difference). All editor Must-Fix bugs closed. Mutation testing clean on encoding and checksum code. |
| **M2 Comms core** | Link trait (serial/J2534/replay/sim), transaction engine, quirk rules, SSM + KWP2000 codecs, ECU simulator, `rr-cli log` | Simulator and replay suites in CI, including fault injection. Live read-only logging works on the target car and cable. |
| **M3 Live data** | Signals and sample frames, wideband devices as profiles, CSV writer, CSV playback, MAF/injector/dyno collectors | All 13 Java devices either have a profile or are listed as dropped. Playback reproduces recorded sessions exactly. |
| **M4 Editor UI** | egui app: ROM tree, N-D table widget, 3D view, compare, undo/redo | Daily-drivable for map editing on the target ROM. |
| **M5 Logger UI** | Dashboards, gauges, charts, live overlay on tables | Daily-drivable for logging sessions. |
| **M6 Breadth & release** | OBD, DS2, NCS (K-line and CAN), ELM327; XDF/ECUFlash import; installers and signed releases | v1.0: parity for supported hardware, release pipeline producing per-OS binaries. |

## Out of scope (for now)

- ECU **flashing** (full reflash). The Java RomRaider does not flash either. Revisit after v1.0, and only with a hardware-in-loop test bench.
- Porting dead or half-finished Java features: the RAM tune test tool, timer proxies, the EvoScan-format `log_defs.xml`, the stub log playback (replaced by CSV-as-producer).
- Platform-SDK devices (Innovate MTS over COM, Phidgets over JNI) until someone with the hardware needs them.

---
*Last reviewed: 2026-10-19*
