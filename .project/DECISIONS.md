# RomRaider-RS — Decisions

Newest first. Each entry has a Title, Date, Status, Context, Decision and Consequences. A new top-level concept (see ARCHITECTURE.md catalogue) **requires** an entry here.

---

## D-028: ECU-definition corpus source: TD-D/SubaruDefs `Alpha`, pinned; logger defs from the forum v370 package, privately mirrored
- **Date:** 2026-10-20 (logger-def fetch amended 2026-10-21)
- **Status:** Accepted
- **Context:**
  - D-025 needs ECU definitions matching the stock-ROM corpus, sourced and pinned the same way (US-RR-16). US-RR-20 also harvests formulas from "the pinned ECU and logger definition corpora". Candidates checked on 2026-10-20:
  - **romraider.com forum, official ECU defs** (https://www.romraider.com/forum/topic360.html, linked from legacy `version.properties:16` `ecudefsurl`): the last zip is v0.8.3.1b of 2009-10-07. The thread's last post (dschultz, RomRaider developer, 2018-02-20) says "Additional definitions, contributions and updates can be found on Github. https://github.com/TD-D/SubaruDefs/tree/Alpha".
  - **https://github.com/TD-D/SubaruDefs**, branch `Alpha`: a fork of Merp/SubaruDefs and the repo the forum points to. RomRaider-format ECU defs live in `RomRaider/ecu/` (`standard/ecu_defs.xml`, `metric/ecu_defs.xml`, `standard/ecu_defs_merpmod.xml`, plus about 250 per-ROM files); ECUFlash-format defs live in `ECUFlash/`; logger defs `RomRaider/logger/logger_*_v290.xml`. Last commit 2024-09-20 (`Alpha` has 100 commits from 2018 to 2024; merged PRs up to 2022). Anyone can fetch it over git without auth. The `RomRaider/` subtree is 229 MB in 267 files, and a few file names contain spaces.
  - **https://github.com/Merp/SubaruDefs** (the original; 197 forks): `Alpha` last commit 2017-10-12; `Stable` holds only the 2012 readme. **https://github.com/RomRaider/SubaruDefs** (fork owned by the RomRaider account): `Alpha` last commit 2013-04-23, and TD-D `Alpha` contains all of it (compare: TD-D 561 ahead, 0 behind). The other forks with recent pushes (Natzirt-BK, qldhighway, ColinDfast1) have only README commits on top of Merp's 2012 `Stable`.
  - **legacy/**: `legacy/definitions/` has no ECU definitions (only `ecu_defs.dtd`, `cars_def.xml`, and a 252-entry sample `log_defs.xml`). Legacy RomRaider requires logger def v370 or later (`version.properties` `min.logger.def.version=370`).
  - **Forum logger defs** (https://www.romraider.com/forum/topic1642.html, legacy `loggerdefsurl`): `logger_v370.zip` (attachment id 38909, updated 2021-11-17), with six `logger_{STD,METRIC,IMP}_{EN,DE}_v370.xml` files. It downloads anonymously only with the forum session cookie and topic referer (a bare request gets a Cloudflare 403). Scripting that cookie/referer bypass was rejected (Ryan, 2026-10-21): it would work around Cloudflare's bot protection, and it would break whenever the forum changes. TD-D carries only v290.
  - **Licence:** none of the GitHub repos has a LICENSE file (GitHub reports none). The README and forum posts contain only a use-at-your-own-risk disclaimer. The forum logger post adds "Please do not host these definitions on your own site. Instead, provide a link to this post only."
  - **Coverage** (throwaway clone at the pin, matched on the manifest's `filename_rom_id`, not on the ID read from the image): 455 of 501 corpus images have a RomRaider-format definition (474 also counting ECUFlash-format). By region: ADM 69/72, EDM 69/77, JDM 99/113, SADM 14/15, USDM 203/222. Every 2004 ADM Liberty in the corpus is covered: the turbo GTs A2WD001G, A2WD010G, A2WD012G, A2ZJ800G, A2ZJC00G, A2ZJD00G and A2ZJD02G, and the naturally aspirated 3.0R D2WD610H. The corpus has no 2004 ADM Liberty 2.0i/2.5i (naturally aspirated 4-cylinder) image, and the definitions have no 2004 ADM Liberty 4-cylinder entry either; the nearest is E2TB102M (2008 ADM Liberty 2.5i).
- **Decision:**
  - The ECU-definition corpus is the `RomRaider/` subtree of https://github.com/TD-D/SubaruDefs, pinned at commit `0b0c0c42acea0d89ffd8b50378ae13ae32b5b43d` (branch `Alpha`, 2024-09-20, tree `ae409e98047c6147c3cc52173c692c3fa0aa35f7`). `ECUFlash/` is not part of the contract corpus; it may be used later as import-only input (ARCHITECTURE.md).
  - The full logger-definition contract is the forum package `logger_v370.zip` (https://www.romraider.com/forum/download/file.php?id=38909), pinned by size 986,211 bytes and SHA-256 `280d386542f6fd49e3467363a62c12c6c642c127f3651a37265bdbac127ea0fa` (Ryan, 2026-10-21).
  - Ryan downloads it **once, manually, in a browser**. No tooling scripts the forum download or any cookie/referer Cloudflare bypass. The copy is kept in a **private** mirror/backup only, like the private ROM backup in D-025.
  - `fetch-corpus` takes the zip from the private mirror or a local path in `RR_CORPUS_DIR` and hash-verifies it against the pin. If it is absent, `fetch-corpus` reports that clearly and carries on (no crash); a hash mismatch is an error.
  - The public tier (and public CI) uses TD-D's v290 logger files from the pinned tree. v370 is the logger contract only when the private copy is present.
  - The TD-D tree is fetched and verified like the ROMs (D-025): `fetch-corpus` downloads it into `RR_CORPUS_DIR`, and the manifest records per-file size and SHA-256 for it and for the v370 zip. Neither is ever committed or redistributed. US-RR-16-5 implements this and US-RR-16-6 reports coverage.
- **Consequences:**
  - No licence is stated, so all rights stay with the authors. We may fetch and use the definitions to test our code but must not vendor, publish or redistribute them. Fetch-and-verify (D-025) is compatible with this, and since nothing is distributed, our GPL-2.0-or-later licence never applies to them. Committed fixtures may contain only metadata such as ROM IDs, paths, hashes and coverage, not definition files, tables or bulk formula lists. US-RR-20 applies the same rule to the harvested formula set: `fixtures/corpus-formulas.toml` holds hashes, counts and locations, never the formula text, which stays in the cache.
  - Individual arithmetic formulas and their constants may appear in tests, KAT `working` strings and test source (Ryan, 2026-10-21). They are functional facts, not definition content: for example `corpus_formulas_known_answers` in `crates/rr-core/src/expr/eval/tests.rs` and the `working` strings in `fixtures/kat/expr-corpus.toml`. The metadata-only rule above still bans committing definition files, tables or bulk formula lists.
  - Following the forum's "do not host" request, the logger zip lives only in a private mirror/backup, like the ROM backup in D-025.
  - Nothing depends on the forum staying reachable or on a Cloudflare bypass. Refreshing v370 (or moving to a newer package) is a manual download plus a manifest pin change.
  - CI fetches from public sources only (Ryan, 2026-10-20), so public CI never has v370: it tests the logger against v290, and v370-only checks must skip with a clear "not present" message rather than fail. Full v370 coverage runs where the private copy is present.
  - As of 2026-10-21 the zip is not yet present locally (earlier scratch copies were Cloudflare error pages); Ryan's manual download is a prerequisite for the v370 tier.
  - Maintenance is slow (last commit 2024-09). Moving the pin is a deliberate manifest change, like the ROM pin.
  - The pinned tree is about 229 MB and some file names contain spaces, so US-RR-16-5 should fetch only the `RomRaider/` subtree and must handle such paths.
  - The 2004 AU Liberty naturally aspirated 4-cylinder (2.0i/2.5i) is in neither corpus. Covering it needs a stock image (for example from Ryan's private fixture corpus) and a definition, which may mean writing one.
  - Implemented by US-RR-16-5 (2026-10-21). The pin lives in the `[defs]` table of the one committed manifest, `fixtures/corpus-manifest.toml` (267 files, 239,467,067 bytes, each with its size and SHA-256; `[defs.logger_v370]` holds the logger pin; the licence position is recorded there too), so the CI cache key covers both corpora. The definitions are cached inside `RR_CORPUS_DIR`, in its top-level `defs/` directory, which no ROM path may use: `defs/romraider/` for the subtree, and `defs/logger_v370.zip` for a hand-placed logger copy. `fetch-corpus` only reads the logger package (`--logger-zip` / `RR_LOGGER_ZIP`, else that path) and never copies it, so it cannot end up in a CI cache. The local mirror is `/home/ryan/Repo/romraider-ecu-defs/`; a private remote backup of it is still to do (Ryan).
  - Coverage reported by US-RR-16-6 (2026-10-21) in `fixtures/corpus-coverage.toml`, generated from the fetched corpus by `cargo xtask gen-corpus-coverage` and checked in CI. Matched on the image bytes as legacy RomRaider matches (`internalidaddress` + `internalidstring`, `base` within the file), 456 of 501 images have a definition (ADM 69/72, EDM 70/77, JDM 99/113, SADM 14/15, USDM 202/222, and both images whose names give no region); 45 have none. Its `filename_rom_id` count reproduces the 455 above region for region. The differences: A8DH2Z1Z's image says `A8DH20  ` and the Baja `A2WC50-A2WC50ZU` image says `A2WC50  `, both matched by bytes only; AE5L500V's only definition (`ecu/AE5L500V_RR.xml`) has a `<rom>` root element, which legacy never matches, so it is covered by name only. The Liberty findings above hold: all eight 2004 ADM Liberty images match, and no 2004 ADM Liberty 4-cylinder exists in either corpus.

## D-027: Coverage gate tolerates a 0.25-point drop
- **Date:** 2026-10-20
- **Status:** Accepted
- **Context:** `cargo xtask coverage-gate` (US-RR-4-5) fails if a gated crate (rr-core, rr-rom, rr-comms) is below 90% line coverage or falls below its coverage on the base. llvm-cov line coverage varies slightly from run to run (inlining, generic instantiation), so a zero-drop rule would fail randomly.
- **Decision:** A crate may fall up to 0.25 points below its base coverage (`--max-drop 0.25`) without failing. The 90% floor still applies.
- **Consequences:** Real regressions are still caught; slow erosion of under 0.25 points per change can pass, but the 90% floor bounds it. If noise turns out larger or smaller in practice, revisit the number here.

## D-026: `u20` (MOVI20) bit 19 sign-extends, matching legacy
- **Date:** 2026-10-20
- **Status:** Accepted
- **Context:** A `u20` raw value is the SH-2A `MOVI20` 20-bit immediate. The CPU and legacy `getMovi20` (`RomAttributeParser.java:181-191`) sign-extend bit 19; the name `u20` suggests unsigned. US-RR-19 left this open until a maintainer decided it.
- **Decision:** Bit 19 sign-extends, so a `u20` value ranges `-0x80000..=0x7FFFF`. The single decision point is `U20_BIT19_SIGN_EXTENDS = true`, in rr-core `encoding.rs` and in the independent reference codec. Legacy `movi20s` (the shifted form) is still refused.
- **Consequences:** Definitions decode `movi20` tables to the same values as legacy RomRaider, so this is not a Must-Fix departure. The synthetic ROM and the `u20-movi20.toml` KAT fixture cover the signed range.

## D-025: External stock-ROM corpus, fetched and pinned, never vendored
- **Date:** 2026-10-19
- **Status:** Accepted
- **Context:**
  - Stock ROMs are the strongest Java-free oracle for checksums and decoding: a stock image's checksums are valid by definition.
  - https://github.com/samuelw9999/RomRaider publicly hosts about 498 stock Subaru ROMs collected from the RomRaider forum (ADM/EDM/JDM/USDM; raw binary, 512 KB and 1 MB images).
  - That repo has no licence, and the images are manufacturer firmware.
- **Decision:**
  - Use it as an external corpus pinned at commit `639f3c73`.
  - `cargo xtask fetch-corpus` downloads it into a cache (`RR_CORPUS_DIR`) and verifies every file against `fixtures/corpus-manifest.toml` (path, size, SHA-256, ROM ID read by our own code).
  - CI caches the corpus and runs the stock-ROM invariant suite on all of it.
  - We never commit or redistribute the images.
  - Matching ECU definitions are sourced and pinned the same way (E2 story).
- **Consequences:**
  - Public CI gets real-world coverage of hundreds of Subaru ROMs on day one, with no private corpus needed.
  - A local mirror was taken on 2026-10-19 at `/home/ryan/Repo/romraider-stock-roms/` (`mirror.git` bare mirror, `roms/` checkout, `SHA256SUMS` for all 501 files). A private backup exists at https://github.com/Biztactix-Ryan/romraider-stock-roms (full mirror, verified against all 501 hashes on 2026-10-19). CI fetches from it first, so an upstream takedown doesn't matter. It must stay private.
  - Two images have non-standard sizes (1,048,731 and 1,048,784 bytes: `JDM/Legacy/AE5I410A…` and one other). They may be CRLF-mangled or have trailing bytes. The manifest marks them `suspect`, and the corpus suite must classify them rather than crash.
  - The manifest also catches corrupted files: that repo uses `* text=auto`, so we verify sizes and hashes.

---

## D-024: No Java in the project, including tests and tooling
- **Date:** 2026-10-19
- **Status:** Accepted. Supersedes the "Java build as oracle" parts of D-001 and D-003.
- **Context:**
  - The original plan used a Java fixture recorder in the repo and a Java build in CI to produce golden outputs.
  - The project's purpose is to leave Java behind.
  - "Matches Java" is also a weak oracle, because the Java code has known bugs (the Must-Fix ledger).
- **Decision:**
  - No Java code is written, built or run by the project: no recorder, no Java CI, no JDK prerequisite.
  - `legacy/` is read-only reference source.
  - Correctness comes from Java-free oracles: hand-checked known-answer fixtures with stated sources; an independent naive reference encoder/decoder in `rr-testkit` that generates synthetic ROMs; stock-ROM checksum invariants; published standard vectors; real car traces captured by our own tooling; agreement between independent definition sources.
  - A developer may run the Java app by hand in a temp directory to compare outputs. That is never committed or gated on.
- **Consequences:**
  - The toolchain is Rust only.
  - Fixtures cost more effort up front (hand-checking, stated sources) but don't inherit Java's bugs.
  - The private stock-ROM corpus becomes more important, because it is the ground truth for checksums.

---

## D-023: Native OpenPort 2.0 Link in Rust (investigate)
- **Date:** 2026-10-19
- **Status:** Proposed. Spike in M2 to confirm feasibility and licence compatibility.
- **Context:**
  - The Tactrix OpenPort 2.0 is the most common RomRaider cable.
  - On Windows it needs Tactrix's J2534 DLL, often 32-bit only.
  - On Linux the Java build ships a third-party `j2534.so` (OpenPort 2.0 over libusb) that must be found at runtime.
  - Both are friction, and both are native code we can't test or fuzz.
- **Decision:** Spike a pure-Rust `OpenPort2` Link using `nusb`, implementing the adapter's USB command protocol directly. Use the existing open-source Linux J2534 implementation as the reference; check its licence is GPL-2.0-or-later compatible before reading its code. If the spike succeeds, OP2 users on every OS need no vendor library. Keep the J2534 Link for other adapters.
- **Consequences:** One fewer native dependency, and the OP2 path becomes fully traceable and testable (T4/T5). Risk: firmware differences between OP2 revisions, so HIL coverage is required before it becomes the default.

## D-022: Linux is a first-class release target
- **Date:** 2026-10-19
- **Status:** Accepted
- **Context:** The Java release treats Linux as an afterthought:
  - a zip plus a `run.sh` hard-coding `lib/linux/32`
  - a system Java on PATH
  - bundled `.so` files
  - users left to sort out serial permissions themselves
- **Decision:**
  - Every release ships AppImage, .deb, .rpm, AUR and tarball (x86_64 + aarch64), built against glibc 2.28.
  - Packaging exists from M0 (nightly `romraider-cli`).
  - udev rules use `uaccess` + ModemManager-ignore, generated from adapter profiles.
  - A `doctor` device check explains and fixes `EACCES`, ModemManager and brltty problems.
  - No bundled native libs; serialport is built without libudev.
  - Package smoke tests run across 7 distros in CI.
  - Binaries are `romraider-rs` / `romraider-cli` (avoiding the `rr` debugger name clash).
- **Consequences:** More release-pipeline work up front. Linux users get a "plug in and go" experience, and aarch64 enables Raspberry Pi in-car logging.

---

## D-021: Drop list: Java features not ported
- **Date:** 2026-10-19
- **Status:** Accepted
- **Context:** The review found dead, half-finished or developer-only code in the Java tree.
- **Decision:** Do not port:
  - `log_defs.xml` (EvoScan format, unreferenced) and `ecu_defs.dtd` (describes a schema nothing reads)
  - `ramtune/` and `util/proxy/*`
  - the log playback stub (hard-coded `foo.csv`), which is replaced by the CSV playback producer
  - the hand-run `TestJ2534*` mains (~2.1k LOC); device mock connections become replay fixtures instead
  - reflection-based gauge styles and the Netscape browser fallback
  - vendored `JFontChooser`
  - Innovate MTS (COM) and Phidgets (JNI) are deferred until someone has the hardware
- **Consequences:** Roughly 5k fewer Java LOC to account for. Anything later found to be needed gets a new decision entry.

## D-020: UI framework: egui/eframe
- **Date:** 2026-10-19
- **Status:** Proposed. Confirm before M4. The alternative is Tauri with a web front end.
- **Context:** The UI is the largest remaining chunk of work: grid-heavy table editing, live gauges, charts, a 3D surface. We want a single binary with no web runtime.
- **Decision:** egui/eframe + `egui_plot`. 3D via an egui_wgpu callback or `three-d`.
- **Consequences:** Pure Rust and one binary. Immediate mode fits live data well. Less polished native look than web/Qt; T10 UI smoke tests via egui_kittest.

## D-019: Errors, logging, and no UI in libraries
- **Date:** 2026-10-19
- **Status:** Accepted
- **Context:** Java has 178 catches of `Exception`, 73 `printStackTrace` calls, NPEs used as control flow, and JOptionPane dialogs raised from model code.
- **Decision:**
  - `thiserror` per crate; `anyhow` only in `rr-cli`/`rr-app`.
  - Recoverable load problems become Diagnostics (C15).
  - `tracing` for logs.
  - Library code never prints, panics on input, or shows UI.
  - Clippy `unwrap_used` is denied outside tests.
- **Consequences:** More explicit error types up front. Every failure is visible and testable.

## D-018: Trace format is the fixture format
- **Date:** 2026-10-19
- **Status:** Accepted
- **Context:** The comms code has no recorded traffic to test against, and debugging relies on scattered hex log lines.
- **Decision:** Every Link writes `.rrtrace` (versioned JSON lines: tx/rx frames, link events, quirks applied, virtual or real timestamps). Replay Links read the same format.
- **Consequences:** Any session on a real car can become a regression test. Users can attach traces to bug reports (after `scrub`).

## D-017: No proprietary ROMs in the public repo
- **Date:** 2026-10-19
- **Status:** Accepted; amended by D-025 (external public corpus, fetched rather than committed)
- **Context:** ECU firmware is manufacturer-copyrighted, but testing needs real ROMs.
- **Decision:** Public CI uses synthetic ROMs generated from definitions. Real ROMs and car traces live in a private corpus (`RR_FIXTURES_DIR`; location `~/RomRaider-private`, outside the repo, backed up with Ryan's home-directory backup: decision, Ryan, 2026-10-20). Pre-commit and CI checks block binaries over 64 KB or known ROM IDs in `fixtures/public`.
- **Consequences:** Two fixture tiers. Full differential coverage only runs where the private corpus is mounted, so a self-hosted runner is needed for it in CI.

## D-016: Licence GPL-2.0-or-later
- **Date:** 2026-10-19
- **Status:** Accepted
- **Context:** RomRaider is licensed "GPL version 2, or (at your option) any later version" (525 of 531 Java file headers say so). A port is a derivative work.
- **Decision:** RomRaider-RS is `GPL-2.0-or-later`. `cargo-deny` licence allowlist is restricted to licences compatible with that (MIT, BSD, ISC, Zlib, Apache-2.0 via GPLv3, MPL-2.0, etc.).
- **Consequences:** Source published with every release. The "or later" clause keeps the common Apache-2.0 / MIT-dual-licensed crate ecosystem usable.

## D-015: One process, single-instance via local socket
- **Date:** 2026-10-19
- **Status:** Accepted
- **Context:** Java runs the editor and logger as cooperating instances over an unauthenticated localhost TCP port (23272), with racy probing and args split on spaces.
- **Decision:** Editor and logger share one process and AppState. A second launch forwards JSON args over a per-user Unix socket or named pipe.
- **Consequences:** The live overlay on tables is a plain channel subscription. Closes MF-36.

## D-014: Typed, versioned TOML settings and profiles with atomic writes
- **Date:** 2026-10-19
- **Status:** Accepted
- **Context:** Java's `Settings` is a 1,000-line global singleton with static fields behind instance setters, hand-written DOM persistence, non-atomic saves, and a CWD override.
- **Decision:**
  - Settings are split into `EditorSettings` / `LoggerSettings` / `UiSettings` (serde, TOML, `schema = N`) under `directories` paths, resolved through Layered (C1): defaults → user file → CLI.
  - Writes are atomic (temp + rename).
  - A one-time importer reads Java `settings.xml`.
  - Settings are passed through an app context; there is no global.
- **Consequences:** Closes MF-35. Schema migrations need tests.

## D-013: Read-only by default; capability-gated writes; no flashing
- **Date:** 2026-10-19
- **Status:** Accepted
- **Context:** The software can brick ECUs. Today, write paths are ordinary method calls.
- **Decision:**
  - ECU writes and ROM file overwrites need capability tokens. Tokens have no public constructor and are only minted by explicit confirmation paths in rr-app or rr-cli.
  - A ROM is never saved with a failing checksum without an explicit, journaled override.
  - Flashing is out of scope until there is a HIL bench.
- **Consequences:** Safety becomes a type-system property plus the T6 test suite. Slightly more ceremony for write features.

## D-012: `Option<f64>` end to end; no silent defaults
- **Date:** 2026-10-19
- **Status:** Accepted
- **Context:** In Java, missing values become 0.0, bad responses re-use the previous good one, and device values have no timestamps (MF-08/09/26).
- **Decision:**
  - SampleFrame values are `Option<f64>`.
  - Signals from devices carry timestamps and go `None` after a TTL.
  - Unknown encodings and out-of-range edits are errors, never 0, wrap or clamp.
- **Consequences:** The UI and CSV have to render gaps. Wrong or stale data can no longer be mistaken for real data.

## D-011: OS threads + bounded channels; no async runtime in the core
- **Date:** 2026-10-19
- **Status:** Accepted
- **Context:** Serial I/O and J2534 DLL calls are blocking. Java uses 42 raw `new Thread`s, non-volatile stop flags and EDT violations.
- **Decision:**
  - One OS thread per live Link.
  - `crossbeam-channel` bounded channels between threads.
  - Stop via cancellation tokens.
  - No `static mut`, no global singletons.
  - The UI reads the latest frame only.
- **Consequences:** Simple, debuggable concurrency. No tokio dependency in core crates.

## D-010: Injected Clock; ban direct sleep and `Instant::now`
- **Date:** 2026-10-19
- **Status:** Accepted
- **Context:** Comms timing is the riskiest behaviour to port, and real-time tests are slow and flaky. Java counts timeouts in sleep iterations (MF-27).
- **Decision:** All timing goes through a `Clock` trait (C9): `RealClock` in production, `VirtualClock` in tests. Clippy `disallowed_methods` bans `std::thread::sleep` and `Instant::now` outside `RealClock`.
- **Consequences:** Timing tests (P1–P4, quirk delays, timeouts) are deterministic and instant. Every timed call site takes a clock.

## D-009: Compile-time registries; no runtime code loading
- **Date:** 2026-10-19
- **Status:** Accepted
- **Context:** Java builds protocols, checksums, J2534 transports, plugins and gauge styles with `Class.forName`, and loads checksum classes from the definition folder (MF-11, an RCE vector). About 30 classes look unused to static search because of this.
- **Decision:** Enums with exhaustive `match`, or explicit registries built at compile time. Data (profiles) selects behaviour; it never names code, except through a fixed set of named hooks.
- **Consequences:** Adding an algorithm or codec means a code change plus a release. In exchange, static analysis sees everything and a definition pack can't execute code.

## D-008: In-house JEP-compatible expression parser, parsed once
- **Date:** 2026-10-19
- **Status:** Accepted
- **Context:**
  - Existing definitions contain JEP-syntax formulas, including `BitWise(...)`, `^` semantics and JEP's function set.
  - Java evaluates them as strings per cell per repaint behind a global lock, and has two inconsistent dialects (MF-25).
  - Generic crates like evalexpr don't match JEP semantics exactly.
- **Decision:**
  - A small Pratt parser in `rr-core::expr` implements the JEP subset used by real definitions (verified against the fixture corpus), plus native bit operators and `[param:unit]` refs.
  - Formulas are parsed once at load into a typed AST.
  - The Conversion enum recognises linear forms for exact inverses.
- **Consequences:** We own the parser, which gets fuzzed and mutation-tested. Compatibility is proven by known-answer fixtures built from formulas in real definitions.

## D-007: One N-D Table type
- **Date:** 2026-10-19
- **Status:** Accepted
- **Context:** Table1D/2D/3D/Switch/Bitwise override every operation two or three times (~950 LOC). `Table3D` hides its parent's `data` (MF-12). The logger has three more near-identical learning-table panels.
- **Decision:** `Table { axes: Vec<Axis>, cells: Grid<CellRef>, kind }` (C11). Cells are views onto bytes. Each operation is written once over a Selection. The same type backs logger learning tables, collector bins and the live overlay.
- **Consequences:** One UI widget, one set of tests. Table kinds become an exhaustive enum.

## D-006: Byte image + Patch journal is the only source of truth
- **Date:** 2026-10-19
- **Status:** Accepted
- **Context:** Java keeps a value object per cell with original and compare copies, syncs overlapping tables through a buggy map (MF-03), and supports revert-only undo.
- **Decision:** The ROM byte image is authoritative. Every mutation, whether a user edit, checksum fix, paste or applied diff, is a `Patch` in a journal (C10).
- **Consequences:** Undo/redo, modified markers, diff, overlap handling and future block-level ECU writes all come from one mechanism.

## D-005: Quirk Rules reused for simulator fault injection
- **Date:** 2026-10-19
- **Status:** Accepted
- **Context:** We need an ECU simulator with fault injection for T5. A bespoke fault DSL would be another concept.
- **Decision:** The simulator applies the same Rule type (C8) on the ECU side. Delays, drops, corruption and silence are actions with the same `when` matching.
- **Consequences:** Faults and quirks share one engine and its tests. A real-world quirk can be reproduced in the simulator by copying the rule.

## D-004: Timing, framing and quirks are layered data, not code
- **Date:** 2026-10-19
- **Status:** Accepted
- **Context:**
  - Java hard-codes protocol timing (e.g. `sleep(55L)`, which is ISO 14230 P3min) and per-ECU special cases.
  - The J2534 path already passes P1/P3/P4 to the adapter, but other paths duplicate the same timings by hand.
- **Decision:**
  - LinkProfiles (baud, Timing P1–P4, Framing, echo, init, recovery policy) are resolved through layers: protocol → adapter → ECU family → ECU ID.
  - Quirks are Rules (`when` → `do`) with a required `id`, `note` and `source`. They can be disabled individually or all at once, and are traced when applied.
  - ECU-specific layers activate after identification, re-resolving.
  - Rules may only change timing, framing, batching and retries, never message semantics. The escape hatch is a named hook.
- **Consequences:** Protocol codecs stay tiny and free of special cases. Supporting a new ECU or cable is a profile file.

## D-003: Fixture-first, multi-tier testing
- **Date:** 2026-10-19
- **Status:** Accepted
- **Context:**
  - Java RomRaider has 10 `@Test` methods (one points at a `C:\Users\...` path) for software that can brick ECUs.
  - There are no sample ROMs, no definitions beyond two tiny ones, and no captured traffic.
- **Decision:**
  - Testing infrastructure is built in M0, before feature code.
  - Expected values come from Java-free oracles (D-024): hand-checked known-answer fixtures, an independent reference decoder, stock-ROM invariants, standard test vectors, real traces and cross-source agreement.
  - Tiers T0–T10 (PROJECT.md): unit, property, differential, fuzz, replay, simulator, safety invariants, mutation, coverage, HIL, UI smoke.
  - Coverage ≥ 90% and zero surviving mutants in safety-critical modules.
  - Every Must-Fix and Edge-Case entry names its proving test.
- **Consequences:** M0 is slower to show visible progress. Every later milestone is checkable. Where we deliberately differ from legacy behaviour, the Must-Fix ledger records why.

## D-002: Unified concept catalogue: minimise distinct structures
- **Date:** 2026-10-19
- **Status:** Accepted
- **Context:**
  - The Java code solves the same problems several times: four kinds of layered configuration, three byte→value decoders, five table classes, 13 copy-paste device plugins, two formula dialects.
  - The user directive is to turn this into as few separate structures and code types as possible.
- **Decision:** ARCHITECTURE.md defines concepts C1–C16. Cross-crate types must use an existing concept. A new concept needs a DECISIONS entry explaining why none fits, plus a catalogue row. Reviews check this.
- **Consequences:** Estimated ~12–15k LOC of Rust logic versus 75k of Java. Learning a dozen concepts unlocks the whole codebase. There is some upfront design cost to get the concepts general enough.

## D-001: Rewrite in Rust inside the fork; Java moved to `legacy/` as read-only reference
- **Date:** 2026-10-19
- **Status:** Accepted
- **Context:**
  - The Java code is dated (Java 6 target, Swing, ~21 MB of vendored libs, JVM required), structurally duplicated and effectively untested.
  - Analysis showed nothing in it, comms included, is technically hard to port. The real risk is validation, not implementation.
  - The user wants to leave Java behind.
- **Decision:**
  - Ground-up Rust rewrite in the fork `Biztactix-Ryan/RomRaider`, on branch `rust-rewrite`.
  - The Java tree moves unchanged into `legacy/` (via `git mv`, history preserved).
  - Eclipse metadata (`.project`, `.classpath`) is deleted; `.project/` now holds the ProjectMan store.
  - The Java CI workflow is removed (D-024).
  - The Cargo workspace lives at the repo root.
  - Upstream Java fixes are merged from the `upstream` remote; rename detection maps them into `legacy/`.
- **Consequences:** The Java source stays available as reference reading, and the root is clean for Rust. Upstream merges touching moved files rely on rename detection; occasional manual conflict resolution is expected.

---
*Last reviewed: 2026-10-19*
