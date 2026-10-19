# Tasks

| ID | Title | Status | Points | Tags | Assignee | Depends On | Story |
| -- | ----- | ------ | ------ | ---- | -------- | ---------- | ----- |
| [US-RR-1-1](tasks/US-RR-1-1.md) | Test: cargo build --workspace and cargo nextest run succeed on a clean checkout with the pinned toolchain | ⚪ todo | — |  | — | — | [US-RR-1](stories/US-RR-1.md) |
| [US-RR-1-2](tasks/US-RR-1-2.md) | Test: A test reading cargo metadata fails if rr-core depends on any workspace crate or on an OS/IO crate (serialport,... | ⚪ todo | — |  | — | — | [US-RR-1](stories/US-RR-1.md) |
| [US-RR-1-3](tasks/US-RR-1-3.md) | Test: A test fails if any crate other than rr-link, rr-cli or rr-app depends on serialport, libloading, nusb or winreg | ⚪ todo | — |  | — | — | [US-RR-1](stories/US-RR-1.md) |
| [US-RR-1-4](tasks/US-RR-1-4.md) | Test: Binary names are romraider-rs and romraider-cli (not rr) and both print --version | ⚪ todo | — |  | — | — | [US-RR-1](stories/US-RR-1.md) |
| [US-RR-10-1](tasks/US-RR-10-1.md) | Test: The dependency-rule test fails if the reference codec module imports anything from rr-core encoding or conversion | ⚪ todo | — |  | — | — | [US-RR-10](stories/US-RR-10.md) |
| [US-RR-10-2](tasks/US-RR-10-2.md) | Test: The reference codec passes the seed known-answer fixtures for every storage type independently of rr-core | ⚪ todo | — |  | — | — | [US-RR-10](stories/US-RR-10.md) |
| [US-RR-10-3](tasks/US-RR-10-3.md) | Test: gen-synthetic output is byte-identical across runs and platforms (CI compares hashes on Linux and Windows) | ⚪ todo | — |  | — | — | [US-RR-10](stories/US-RR-10.md) |
| [US-RR-10-4](tasks/US-RR-10-4.md) | Test: Generated ROMs exercise overlapping tables, tables ending exactly at ROM end, NaN/Inf/denormal floats and min/m... | ⚪ todo | — |  | — | — | [US-RR-10](stories/US-RR-10.md) |
| [US-RR-11-1](tasks/US-RR-11-1.md) | Test: fetch-corpus verifies all 501 files against the committed manifest and fails loudly on any hash or size mismatch | ⚪ todo | — |  | — | — | [US-RR-11](stories/US-RR-11.md) |
| [US-RR-11-2](tasks/US-RR-11-2.md) | Test: Images with non-power-of-two or non-standard sizes are marked suspect in the manifest with a reason, and tests ... | ⚪ todo | — |  | — | — | [US-RR-11](stories/US-RR-11.md) |
| [US-RR-11-3](tasks/US-RR-11-3.md) | Test: No ROM bytes are ever written inside the repository working tree (test asserts RR_CORPUS_DIR is outside the repo) | ⚪ todo | — |  | — | — | [US-RR-11](stories/US-RR-11.md) |
| [US-RR-11-4](tasks/US-RR-11-4.md) | Test: CI caches the corpus keyed by manifest hash and a cold fetch completes within the CI time budget | ⚪ todo | — |  | — | — | [US-RR-11](stories/US-RR-11.md) |
| [US-RR-12-1](tasks/US-RR-12-1.md) | Test: Every area listed has a docs/legacy page with legacy/ file:line references for each described behaviour | ⚪ todo | — |  | — | — | [US-RR-12](stories/US-RR-12.md) |
| [US-RR-12-2](tasks/US-RR-12-2.md) | Test: Each page ends with a fixtures-to-write list, and each listed fixture becomes a tracked task or KAT file | ⚪ todo | — |  | — | — | [US-RR-12](stories/US-RR-12.md) |
| [US-RR-12-3](tasks/US-RR-12-3.md) | Test: Behaviours that are bugs are cross-referenced to an MF ledger entry rather than described as intended | ⚪ todo | — |  | — | — | [US-RR-12](stories/US-RR-12.md) |
| [US-RR-12-4](tasks/US-RR-12-4.md) | Test: The SSM page documents init, read-address, read-memory, write and echo handling precisely enough to write simul... | ⚪ todo | — |  | — | — | [US-RR-12](stories/US-RR-12.md) |
| [US-RR-13-1](tasks/US-RR-13-1.md) | Test: Attempting to commit a stock ROM from the corpus under fixtures/public is blocked by the pre-commit hook and by CI | ⚪ todo | — |  | — | — | [US-RR-13](stories/US-RR-13.md) |
| [US-RR-13-2](tasks/US-RR-13-2.md) | Test: A trace containing a 17-character VIN pattern under fixtures/public fails CI | ⚪ todo | — |  | — | — | [US-RR-13](stories/US-RR-13.md) |
| [US-RR-13-3](tasks/US-RR-13-3.md) | Test: Private-fixture tests run when RR_FIXTURES_DIR is set and report skipped otherwise | ⚪ todo | — |  | — | — | [US-RR-13](stories/US-RR-13.md) |
| [US-RR-13-4](tasks/US-RR-13-4.md) | Test: The guard itself has tests with planted positive and negative samples | ⚪ todo | — |  | — | — | [US-RR-13](stories/US-RR-13.md) |
| [US-RR-14-1](tasks/US-RR-14-1.md) | Test: Every vector fixture cites its standard document, edition and section (or legacy file:line for the transcribed ... | ⚪ todo | — |  | — | — | [US-RR-14](stories/US-RR-14.md) |
| [US-RR-14-2](tasks/US-RR-14-2.md) | Test: ISO 14230 default timings (including P3min = 55 ms) are exposed as named constants backed by a fixture, and MF-... | ⚪ todo | — |  | — | — | [US-RR-14](stories/US-RR-14.md) |
| [US-RR-14-3](tasks/US-RR-14-3.md) | Test: Checksum reference vectors cover each sum/XOR/CRC variant planned for the checksum engine | ⚪ todo | — |  | — | — | [US-RR-14](stories/US-RR-14.md) |
| [US-RR-14-4](tasks/US-RR-14-4.md) | Test: J1979 vectors include boundary values (0, max) for every included PID | ⚪ todo | — |  | — | — | [US-RR-14](stories/US-RR-14.md) |
| [US-RR-15-1](tasks/US-RR-15-1.md) | Test: Given two definition sources for the same synthetic ROM, the harness reports zero disagreements when they agree... | ⚪ todo | — |  | — | — | [US-RR-15](stories/US-RR-15.md) |
| [US-RR-15-2](tasks/US-RR-15-2.md) | Test: Tolerances are per unit and explicit; a zero-tolerance default applies to raw values | ⚪ todo | — |  | — | — | [US-RR-15](stories/US-RR-15.md) |
| [US-RR-15-3](tasks/US-RR-15-3.md) | Test: Unmatched tables in either source are reported, not silently skipped | ⚪ todo | — |  | — | — | [US-RR-15](stories/US-RR-15.md) |
| [US-RR-16-1](tasks/US-RR-16-1.md) | Test: The definition source, its licence and the pinned version are recorded in DECISIONS.md and the manifest | ⚪ todo | — |  | — | — | [US-RR-16](stories/US-RR-16.md) |
| [US-RR-16-2](tasks/US-RR-16-2.md) | Test: fetch-corpus fetches and hash-verifies the definitions alongside the ROMs, and a local mirror exists | ⚪ todo | — |  | — | — | [US-RR-16](stories/US-RR-16.md) |
| [US-RR-16-3](tasks/US-RR-16-3.md) | Test: A coverage report lists every corpus ROM ID with its matching definition or 'no definition', committed as a fix... | ⚪ todo | — |  | — | — | [US-RR-16](stories/US-RR-16.md) |
| [US-RR-17-1](tasks/US-RR-17-1.md) | Test: Diagnostic codes are unique and stable - a test fails if a code is reused or removed without a deprecation entry | ⚪ todo | — |  | — | — | [US-RR-17](stories/US-RR-17.md) |
| [US-RR-17-2](tasks/US-RR-17-2.md) | Test: Provenance renders as file:line:col for text sources and as an offset for binary sources, with snapshot tests | ⚪ todo | — |  | — | — | [US-RR-17](stories/US-RR-17.md) |
| [US-RR-17-3](tasks/US-RR-17-3.md) | Test: Severity Error diagnostics make the owning load fail closed (the caller cannot get a usable T without explicitl... | ⚪ todo | — |  | — | — | [US-RR-17](stories/US-RR-17.md) |
| [US-RR-18-1](tasks/US-RR-18-1.md) | Test: VirtualClock: sleeping 55 ms then 5000 ms advances virtual time exactly and the test completes in under 10 ms o... | ⚪ todo | — |  | — | — | [US-RR-18](stories/US-RR-18.md) |
| [US-RR-18-2](tasks/US-RR-18-2.md) | Test: Two threads sharing a VirtualClock observe a consistent monotonic timeline (property test) | ⚪ todo | — |  | — | — | [US-RR-18](stories/US-RR-18.md) |
| [US-RR-18-3](tasks/US-RR-18-3.md) | Test: Monotonic time never goes backwards even if wall-clock is changed during a RealClock test (wall time is reporte... | ⚪ todo | — |  | — | — | [US-RR-18](stories/US-RR-18.md) |
| [US-RR-18-4](tasks/US-RR-18-4.md) | Test: RealClock is the only module allowed to call std time APIs (enforced by the safety lints story) | ⚪ todo | — |  | — | — | [US-RR-18](stories/US-RR-18.md) |
| [US-RR-19-1](tasks/US-RR-19-1.md) | Test: Property test: decode(encode(x)) == x for every storage/endian/bitfield/layout combination over the full value ... | ⚪ todo | — |  | — | — | [US-RR-19](stories/US-RR-19.md) |
| [US-RR-19-2](tasks/US-RR-19-2.md) | Test: U24, I24 and I64 values decode correctly; an unsupported storage string in a definition produces an Error diagn... | ⚪ todo | — |  | — | — | [US-RR-19](stories/US-RR-19.md) |
| [US-RR-19-3](tasks/US-RR-19-3.md) | Test: Two images with different endianness decode correctly in the same process concurrently (MF-04) | ⚪ todo | — |  | — | — | [US-RR-19](stories/US-RR-19.md) |
| [US-RR-19-4](tasks/US-RR-19-4.md) | Test: Float min/max ranges are derived from the Encoding and are correct for F32/F64 signed and unsigned contexts (MF... | ⚪ todo | — |  | — | — | [US-RR-19](stories/US-RR-19.md) |
| [US-RR-19-5](tasks/US-RR-19-5.md) | Test: NaN, +/-Inf and denormal floats decode faithfully and are flagged so edit layers can refuse them (EC-07) | ⚪ todo | — |  | — | — | [US-RR-19](stories/US-RR-19.md) |
| [US-RR-19-6](tasks/US-RR-19-6.md) | Test: cargo-mutants reports zero surviving mutants in the encoding module | ⚪ todo | — |  | — | — | [US-RR-19](stories/US-RR-19.md) |
| [US-RR-2-1](tasks/US-RR-2-1.md) | Test: A PR with a rustfmt violation, a clippy warning, or a failing test is blocked on every matrix target | ⚪ todo | — |  | — | — | [US-RR-2](stories/US-RR-2.md) |
| [US-RR-2-2](tasks/US-RR-2-2.md) | Test: cargo-deny fails the build for a dependency with a GPL-incompatible licence or an open RUSTSEC advisory (demons... | ⚪ todo | — |  | — | — | [US-RR-2](stories/US-RR-2.md) |
| [US-RR-2-3](tasks/US-RR-2-3.md) | Test: A PR that leaves pending insta snapshots fails CI | ⚪ todo | — |  | — | — | [US-RR-2](stories/US-RR-2.md) |
| [US-RR-2-4](tasks/US-RR-2-4.md) | Test: The i686-pc-windows-msvc target builds and runs the test suite | ⚪ todo | — |  | — | — | [US-RR-2](stories/US-RR-2.md) |
| [US-RR-20-1](tasks/US-RR-20-1.md) | Test: Every distinct formula in the pinned ECU and logger definition corpora parses, and a sample of at least 50 has ... | ⚪ todo | — |  | — | — | [US-RR-20](stories/US-RR-20.md) |
| [US-RR-20-2](tasks/US-RR-20-2.md) | Test: JEP operator precedence and ^ semantics match documented JEP behaviour via KAT fixtures citing the JEP document... | ⚪ todo | — |  | — | — | [US-RR-20](stories/US-RR-20.md) |
| [US-RR-20-3](tasks/US-RR-20-3.md) | Test: The expr fuzz target runs nightly with no panics, hangs or unbounded allocation; depth and size limits produce ... | ⚪ todo | — |  | — | — | [US-RR-20](stories/US-RR-20.md) |
| [US-RR-20-4](tasks/US-RR-20-4.md) | Test: Evaluation of a parsed AST performs no allocation and no locking (benchmark + test), fixing the global synchron... | ⚪ todo | — |  | — | — | [US-RR-20](stories/US-RR-20.md) |
| [US-RR-20-5](tasks/US-RR-20-5.md) | Test: cargo-mutants reports zero surviving mutants in the evaluator | ⚪ todo | — |  | — | — | [US-RR-20](stories/US-RR-20.md) |
| [US-RR-21-1](tasks/US-RR-21-1.md) | Test: Every conversion factor has a KAT fixture citing an SI/NIST or equivalent reference (e.g. km per mile = 1.60934... | ⚪ todo | — |  | — | — | [US-RR-21](stories/US-RR-21.md) |
| [US-RR-21-2](tasks/US-RR-21-2.md) | Test: Property test: converting A->B->A round-trips within floating tolerance for every unit pair in the same dimension | ⚪ todo | — |  | — | — | [US-RR-21](stories/US-RR-21.md) |
| [US-RR-21-3](tasks/US-RR-21-3.md) | Test: Every unit string appearing in the pinned logger and ECU definition corpora resolves to a registry unit or prod... | ⚪ todo | — |  | — | — | [US-RR-21](stories/US-RR-21.md) |
| [US-RR-22-1](tasks/US-RR-22-1.md) | Test: Property tests: resolution is deterministic and idempotent; overriding a field in a child always wins over the ... | ⚪ todo | — |  | — | — | [US-RR-22](stories/US-RR-22.md) |
| [US-RR-22-2](tasks/US-RR-22-2.md) | Test: Omitting one item in the middle of a list removes only that item and keeps all later items (MF-07) | ⚪ todo | — |  | — | — | [US-RR-22](stories/US-RR-22.md) |
| [US-RR-22-3](tasks/US-RR-22-3.md) | Test: A cycle across files produces an Error diagnostic naming the full cycle path - never a stack overflow (MF-17); ... | ⚪ todo | — |  | — | — | [US-RR-22](stories/US-RR-22.md) |
| [US-RR-22-4](tasks/US-RR-22-4.md) | Test: A name defined in a local scope is not visible as a global base (MF-16) | ⚪ todo | — |  | — | — | [US-RR-22](stories/US-RR-22.md) |
| [US-RR-22-5](tasks/US-RR-22-5.md) | Test: explain(field) reports the winning layer and the overridden values with provenance | ⚪ todo | — |  | — | — | [US-RR-22](stories/US-RR-22.md) |
| [US-RR-22-6](tasks/US-RR-22-6.md) | Test: cargo-mutants reports zero surviving mutants in the resolver | ⚪ todo | — |  | — | — | [US-RR-22](stories/US-RR-22.md) |
| [US-RR-23-1](tasks/US-RR-23-1.md) | Test: Property test: a byte pattern compiled from a concrete byte string with random wildcards matches that string an... | ⚪ todo | — |  | — | — | [US-RR-23](stories/US-RR-23.md) |
| [US-RR-23-2](tasks/US-RR-23-2.md) | Test: Pattern text parsing is fuzzed with no panics; malformed patterns produce diagnostics | ⚪ todo | — |  | — | — | [US-RR-23](stories/US-RR-23.md) |
| [US-RR-23-3](tasks/US-RR-23-3.md) | Test: Matching at an offset past the end of the input returns no-match, never panics | ⚪ todo | — |  | — | — | [US-RR-23](stories/US-RR-23.md) |
| [US-RR-24-1](tasks/US-RR-24-1.md) | Test: Property test: for every invertible conversion and every raw value r, to_raw(to_real(r)) == r exactly for linea... | ⚪ todo | — |  | — | — | [US-RR-24](stories/US-RR-24.md) |
| [US-RR-24-2](tasks/US-RR-24-2.md) | Test: Decreasing scalings report correct min/max display ranges (MF-13) - KAT fixtures with hand-calculated ranges | ⚪ todo | — |  | — | — | [US-RR-24](stories/US-RR-24.md) |
| [US-RR-24-3](tasks/US-RR-24-3.md) | Test: A non-monotonic conversion with no to_raw is rejected for editing with a diagnostic; 4-byte storage inverse nev... | ⚪ todo | — |  | — | — | [US-RR-24](stories/US-RR-24.md) |
| [US-RR-24-4](tasks/US-RR-24-4.md) | Test: A target real value outside the representable range, or hitting a division-by-zero region, is rejected with a r... | ⚪ todo | — |  | — | — | [US-RR-24](stories/US-RR-24.md) |
| [US-RR-24-5](tasks/US-RR-24-5.md) | Test: The same input always yields the same output regardless of call order (no cache-dependent results) - property t... | ⚪ todo | — |  | — | — | [US-RR-24](stories/US-RR-24.md) |
| [US-RR-24-6](tasks/US-RR-24-6.md) | Test: cargo-mutants reports zero surviving mutants in the conversion module | ⚪ todo | — |  | — | — | [US-RR-24](stories/US-RR-24.md) |
| [US-RR-25-1](tasks/US-RR-25-1.md) | Test: Property test: any sequence of valid frames, split at arbitrary byte boundaries and fed incrementally, yields e... | ⚪ todo | — |  | — | — | [US-RR-25](stories/US-RR-25.md) |
| [US-RR-25-2](tasks/US-RR-25-2.md) | Test: Garbage before a header, two frames in one read, and a frame split across reads are all handled (EC-17) | ⚪ todo | — |  | — | — | [US-RR-25](stories/US-RR-25.md) |
| [US-RR-25-3](tasks/US-RR-25-3.md) | Test: A Sync frame failing its checksum is dropped and reported, never emitted as data (MF-10) | ⚪ todo | — |  | — | — | [US-RR-25](stories/US-RR-25.md) |
| [US-RR-25-4](tasks/US-RR-25-4.md) | Test: Every variant has a fuzz target with no panics or unbounded buffering | ⚪ todo | — |  | — | — | [US-RR-25](stories/US-RR-25.md) |
| [US-RR-26-1](tasks/US-RR-26-1.md) | Test: A rule without id, note or source is rejected at load with a diagnostic | ⚪ todo | — |  | — | — | [US-RR-26](stories/US-RR-26.md) |
| [US-RR-26-2](tasks/US-RR-26-2.md) | Test: Layer precedence (protocol < adapter < ECU family < ECU ID) is honoured for conflicting actions - property test... | ⚪ todo | — |  | — | — | [US-RR-26](stories/US-RR-26.md) |
| [US-RR-26-3](tasks/US-RR-26-3.md) | Test: disable-one and disable-all remove exactly the targeted rules, verified by trace output | ⚪ todo | — |  | — | — | [US-RR-26](stories/US-RR-26.md) |
| [US-RR-26-4](tasks/US-RR-26-4.md) | Test: Applying rules to an op with no matching rules costs a single table lookup (benchmark asserts no scan) | ⚪ todo | — |  | — | — | [US-RR-26](stories/US-RR-26.md) |
| [US-RR-26-5](tasks/US-RR-26-5.md) | Test: Unknown hook names are a load error, not a runtime surprise | ⚪ todo | — |  | — | — | [US-RR-26](stories/US-RR-26.md) |
| [US-RR-26-6](tasks/US-RR-26-6.md) | Test: cargo-mutants reports zero surviving mutants in rule flattening and application | ⚪ todo | — |  | — | — | [US-RR-26](stories/US-RR-26.md) |
| [US-RR-27-1](tasks/US-RR-27-1.md) | Test: Billion-laughs, external-entity (XXE) and deeply-nested inputs are rejected with diagnostics within bounded tim... | ⚪ todo | — |  | — | — | [US-RR-27](stories/US-RR-27.md) |
| [US-RR-27-2](tasks/US-RR-27-2.md) | Test: ISO-8859-1, UTF-8 with BOM and CRLF variants of the same definition parse to identical results | ⚪ todo | — |  | — | — | [US-RR-27](stories/US-RR-27.md) |
| [US-RR-27-3](tasks/US-RR-27-3.md) | Test: The XML fuzz target runs nightly with no panics, hangs or unbounded allocation | ⚪ todo | — |  | — | — | [US-RR-27](stories/US-RR-27.md) |
| [US-RR-27-4](tasks/US-RR-27-4.md) | Test: Every pinned definition file in the corpus parses without Error diagnostics | ⚪ todo | — |  | — | — | [US-RR-27](stories/US-RR-27.md) |
| [US-RR-28-1](tasks/US-RR-28-1.md) | Test: Every schema round-trips: load(save(x)) == x (property test) | ⚪ todo | — |  | — | — | [US-RR-28](stories/US-RR-28.md) |
| [US-RR-28-2](tasks/US-RR-28-2.md) | Test: A profile with an older schema version migrates to the current one with a snapshot test per migration step; an ... | ⚪ todo | — |  | — | — | [US-RR-28](stories/US-RR-28.md) |
| [US-RR-28-3](tasks/US-RR-28-3.md) | Test: Unknown keys are errors with line:col provenance (no silent ignore) | ⚪ todo | — |  | — | — | [US-RR-28](stories/US-RR-28.md) |
| [US-RR-28-4](tasks/US-RR-28-4.md) | Test: TOML profile parsing is fuzzed with no panics | ⚪ todo | — |  | — | — | [US-RR-28](stories/US-RR-28.md) |
| [US-RR-29-1](tasks/US-RR-29-1.md) | Test: Every definition file in the pinned definition corpus loads; any Error diagnostic is either fixed or recorded a... | ⚪ todo | — |  | — | — | [US-RR-29](stories/US-RR-29.md) |
| [US-RR-29-2](tasks/US-RR-29-2.md) | Test: Each table type has a hand-checked KAT fixture: minimal XML in, expected typed layered doc out | ⚪ todo | — |  | — | — | [US-RR-29](stories/US-RR-29.md) |
| [US-RR-29-3](tasks/US-RR-29-3.md) | Test: Unknown elements or attributes produce Warning diagnostics naming them with line:col (snapshot tested) | ⚪ todo | — |  | — | — | [US-RR-29](stories/US-RR-29.md) |
| [US-RR-29-4](tasks/US-RR-29-4.md) | Test: The reader never panics on any fuzzed definition input | ⚪ todo | — |  | — | — | [US-RR-29](stories/US-RR-29.md) |
| [US-RR-3-1](tasks/US-RR-3-1.md) | Test: A test crate calling std::thread::sleep or Instant::now outside RealClock fails clippy in CI | ⚪ todo | — |  | — | — | [US-RR-3](stories/US-RR-3.md) |
| [US-RR-3-2](tasks/US-RR-3-2.md) | Test: unwrap() or expect() in non-test library code fails clippy | ⚪ todo | — |  | — | — | [US-RR-3](stories/US-RR-3.md) |
| [US-RR-3-3](tasks/US-RR-3-3.md) | Test: A static mut or a mutable global singleton in any workspace crate fails CI (clippy or a grep-based check with a... | ⚪ todo | — |  | — | — | [US-RR-3](stories/US-RR-3.md) |
| [US-RR-3-4](tasks/US-RR-3-4.md) | Test: The lint configuration is documented in CONTRIBUTING with the reason for each ban | ⚪ todo | — |  | — | — | [US-RR-3](stories/US-RR-3.md) |
| [US-RR-30-1](tasks/US-RR-30-1.md) | Test: Given SSM init bytes captured from a real ECU (fixture), resolve() yields exactly the parameters whose capabili... | ⚪ todo | — |  | — | — | [US-RR-30](stories/US-RR-30.md) |
| [US-RR-30-2](tasks/US-RR-30-2.md) | Test: A derived parameter with a missing dependency produces a diagnostic naming the dependency and stays visible as ... | ⚪ todo | — |  | — | — | [US-RR-30](stories/US-RR-30.md) |
| [US-RR-30-3](tasks/US-RR-30-3.md) | Test: Every parameter used by MAF/injector/dyno collectors has a role assigned for SSM (MF-31) | ⚪ todo | — |  | — | — | [US-RR-30](stories/US-RR-30.md) |
| [US-RR-30-4](tasks/US-RR-30-4.md) | Test: Logger definition parsing is fuzzed with no panics | ⚪ todo | — |  | — | — | [US-RR-30](stories/US-RR-30.md) |
| [US-RR-31-1](tasks/US-RR-31-1.md) | Test: Property test: for any sequence of patches, undo-all restores the original bytes exactly and redo-all reproduce... | ⚪ todo | — |  | — | — | [US-RR-31](stories/US-RR-31.md) |
| [US-RR-31-2](tasks/US-RR-31-2.md) | Test: A patch whose old bytes do not match the current image is rejected (stale-patch protection) | ⚪ todo | — |  | — | — | [US-RR-31](stories/US-RR-31.md) |
| [US-RR-31-3](tasks/US-RR-31-3.md) | Test: The sidecar journal round-trips losslessly (property test) and rejects a journal whose base image hash does not... | ⚪ todo | — |  | — | — | [US-RR-31](stories/US-RR-31.md) |
| [US-RR-31-4](tasks/US-RR-31-4.md) | Test: There is no public API that mutates image bytes without producing a patch - enforced by module visibility and a... | ⚪ todo | — |  | — | — | [US-RR-31](stories/US-RR-31.md) |
| [US-RR-31-5](tasks/US-RR-31-5.md) | Test: cargo-mutants reports zero surviving mutants in the journal module | ⚪ todo | — |  | — | — | [US-RR-31](stories/US-RR-31.md) |
| [US-RR-32-1](tasks/US-RR-32-1.md) | Test: A table in docs/legacy lists all 17 sleeps with legacy file:line, classification (standard timing / quirk / pol... | ⚪ todo | — |  | — | — | [US-RR-32](stories/US-RR-32.md) |
| [US-RR-32-2](tasks/US-RR-32-2.md) | Test: Every standard timing in the base profiles references a standard-vector fixture; every quirk rule has id, note ... | ⚪ todo | — |  | — | — | [US-RR-32](stories/US-RR-32.md) |
| [US-RR-32-3](tasks/US-RR-32-3.md) | Test: Polling artefacts (sleep(1)/sleep(2) loops) are explicitly marked as not ported, with the replacement mechanism... | ⚪ todo | — |  | — | — | [US-RR-32](stories/US-RR-32.md) |
| [US-RR-33-1](tasks/US-RR-33-1.md) | Test: Property test: recording any sequence of transactions through TraceWriter then replaying via ReplayLink reprodu... | ⚪ todo | — |  | — | — | [US-RR-33](stories/US-RR-33.md) |
| [US-RR-33-2](tasks/US-RR-33-2.md) | Test: ReplayLink fails the test with a precise diff when the code under test sends a request that differs from the trace | ⚪ todo | — |  | — | — | [US-RR-33](stories/US-RR-33.md) |
| [US-RR-33-3](tasks/US-RR-33-3.md) | Test: The .rrtrace reader is fuzzed with no panics; an unknown trace version is rejected | ⚪ todo | — |  | — | — | [US-RR-33](stories/US-RR-33.md) |
| [US-RR-33-4](tasks/US-RR-33-4.md) | Test: Traces carry no VIN or serial unless explicitly marked unscrubbed (field-level marking for the scrub command) | ⚪ todo | — |  | — | — | [US-RR-33](stories/US-RR-33.md) |
| [US-RR-34-1](tasks/US-RR-34-1.md) | Test: Every non-suspect stock ROM in the external corpus that has a definition resolves to exactly one definition; re... | ⚪ todo | — |  | — | — | [US-RR-34](stories/US-RR-34.md) |
| [US-RR-34-2](tasks/US-RR-34-2.md) | Test: A ROM matching two definitions is refused with a diagnostic listing both candidates and no definition is applie... | ⚪ todo | — |  | — | — | [US-RR-34](stories/US-RR-34.md) |
| [US-RR-34-3](tasks/US-RR-34-3.md) | Test: A ROM whose size differs from the definition's declared size produces an Error diagnostic and editing is disabled | ⚪ todo | — |  | — | — | [US-RR-34](stories/US-RR-34.md) |
| [US-RR-34-4](tasks/US-RR-34-4.md) | Test: Benchmarks enforce the resolve (<200 ms for 5k tables) and lookup (<5 ms) budgets in CI (MF-18, MF-40, EC-10) | ⚪ todo | — |  | — | — | [US-RR-34](stories/US-RR-34.md) |
| [US-RR-34-5](tasks/US-RR-34-5.md) | Test: Resolved Definitions are immutable (no interior mutability) - compile-time and review check | ⚪ todo | — |  | — | — | [US-RR-34](stories/US-RR-34.md) |
| [US-RR-35-1](tasks/US-RR-35-1.md) | Test: Every non-suspect stock ROM in the external corpus validates with zero failures under its family's checksum spec | ⚪ todo | — |  | — | — | [US-RR-35](stories/US-RR-35.md) |
| [US-RR-35-2](tasks/US-RR-35-2.md) | Test: compute() on an unmodified stock ROM returns zero patches; after flipping any single data byte in a checksummed... | ⚪ todo | — |  | — | — | [US-RR-35](stories/US-RR-35.md) |
| [US-RR-35-3](tasks/US-RR-35-3.md) | Test: When any segment's validation fails (e.g. E38 segment mismatch), compute returns an error and zero patches - ne... | ⚪ todo | — |  | — | — | [US-RR-35](stories/US-RR-35.md) |
| [US-RR-35-4](tasks/US-RR-35-4.md) | Test: Checksums that depend on other checksums are computed in declared order and the result is order-independent of ... | ⚪ todo | — |  | — | — | [US-RR-35](stories/US-RR-35.md) |
| [US-RR-35-5](tasks/US-RR-35-5.md) | Test: An unknown named algorithm is an Error diagnostic at load; there is no code path that loads algorithms from fil... | ⚪ todo | — |  | — | — | [US-RR-35](stories/US-RR-35.md) |
| [US-RR-35-6](tasks/US-RR-35-6.md) | Test: The Subaru checksum is driven by the ChecksumSpec, not by a table named 'Checksum Fix' (MF-19) | ⚪ todo | — |  | — | — | [US-RR-35](stories/US-RR-35.md) |
| [US-RR-35-7](tasks/US-RR-35-7.md) | Test: cargo-mutants reports zero surviving mutants in the checksum engine and every registry algorithm | ⚪ todo | — |  | — | — | [US-RR-35](stories/US-RR-35.md) |
| [US-RR-36-1](tasks/US-RR-36-1.md) | Test: The simulator answers SSM init with configurable ECU ID/capability bytes and serves read-address and read-memor... | ⚪ todo | — |  | — | — | [US-RR-36](stories/US-RR-36.md) |
| [US-RR-36-2](tasks/US-RR-36-2.md) | Test: Each fault action from the Rule set is demonstrated by a test that observes the fault on the wire via trace | ⚪ todo | — |  | — | — | [US-RR-36](stories/US-RR-36.md) |
| [US-RR-36-3](tasks/US-RR-36-3.md) | Test: Every write request received is recorded with token metadata, and a helper asserts zero unauthorised writes | ⚪ todo | — |  | — | — | [US-RR-36](stories/US-RR-36.md) |
| [US-RR-36-4](tasks/US-RR-36-4.md) | Test: A 10-minute simulated logging session with faults runs in under 2 seconds of real time on VirtualClock | ⚪ todo | — |  | — | — | [US-RR-36](stories/US-RR-36.md) |
| [US-RR-37-1](tasks/US-RR-37-1.md) | Test: Using a pty pair (Linux) / com0com-style harness, echo bytes are stripped and an echo mismatch is reported as a... | ⚪ todo | — |  | — | — | [US-RR-37](stories/US-RR-37.md) |
| [US-RR-37-2](tasks/US-RR-37-2.md) | Test: Removing the device mid-transaction yields a Link error within the configured deadline, with no hang or panic (... | ⚪ todo | — |  | — | — | [US-RR-37](stories/US-RR-37.md) |
| [US-RR-37-3](tasks/US-RR-37-3.md) | Test: After replug on a different /dev/ttyUSBn the link reconnects by stable ID (EC-19c) | ⚪ todo | — |  | — | — | [US-RR-37](stories/US-RR-37.md) |
| [US-RR-37-4](tasks/US-RR-37-4.md) | Test: The FTDI latency timer value is detected on Linux and a Warning diagnostic is raised if it is above 1 ms | ⚪ todo | — |  | — | — | [US-RR-37](stories/US-RR-37.md) |
| [US-RR-37-5](tasks/US-RR-37-5.md) | Test: HIL: opening the KKL cable at 4800 8N1 against the Liberty succeeds (recorded in the HIL report) | ⚪ todo | — |  | — | — | [US-RR-37](stories/US-RR-37.md) |
| [US-RR-38-1](tasks/US-RR-38-1.md) | Test: The nightly workflow publishes a pre-release with tarball, AppImage, .deb and SHA256SUMS | ⚪ todo | — |  | — | — | [US-RR-38](stories/US-RR-38.md) |
| [US-RR-38-2](tasks/US-RR-38-2.md) | Test: A CI job installs the .deb in a clean Ubuntu container and runs romraider-cli --version successfully | ⚪ todo | — |  | — | — | [US-RR-38](stories/US-RR-38.md) |
| [US-RR-38-3](tasks/US-RR-38-3.md) | Test: The AppImage runs romraider-cli --version on a clean Ubuntu 22.04 container | ⚪ todo | — |  | — | — | [US-RR-38](stories/US-RR-38.md) |
| [US-RR-38-4](tasks/US-RR-38-4.md) | Test: Release artifacts never contain ROM images or fixture data (artifact content check) | ⚪ todo | — |  | — | — | [US-RR-38](stories/US-RR-38.md) |
| [US-RR-39-1](tasks/US-RR-39-1.md) | Test: Each listed inheritance case has a KAT fixture with a stated hand-calc source and passes | ⚪ todo | — |  | — | — | [US-RR-39](stories/US-RR-39.md) |
| [US-RR-39-2](tasks/US-RR-39-2.md) | Test: Resolved-tree snapshots for 20 real corpus definitions are committed with a review note and any change requires... | ⚪ todo | — |  | — | — | [US-RR-39](stories/US-RR-39.md) |
| [US-RR-39-3](tasks/US-RR-39-3.md) | Test: Each MF entry listed is referenced from the fixture that proves it, and the ledger links back to the fixture | ⚪ todo | — |  | — | — | [US-RR-39](stories/US-RR-39.md) |
| [US-RR-4-1](tasks/US-RR-4-1.md) | Test: A PR that drops coverage of rr-core/rr-rom/rr-comms below 90% or below the base branch fails | ⚪ todo | — |  | — | — | [US-RR-4](stories/US-RR-4.md) |
| [US-RR-4-2](tasks/US-RR-4-2.md) | Test: A deliberately weakened test on a gated module produces a surviving mutant and fails the mutants job | ⚪ todo | — |  | — | — | [US-RR-4](stories/US-RR-4.md) |
| [US-RR-4-3](tasks/US-RR-4-3.md) | Test: Every exclusion in mutants.toml has a justification comment and a CI check rejects uncommented exclusions | ⚪ todo | — |  | — | — | [US-RR-4](stories/US-RR-4.md) |
| [US-RR-4-4](tasks/US-RR-4-4.md) | Test: The weekly full mutation run publishes its report as a CI artifact | ⚪ todo | — |  | — | — | [US-RR-4](stories/US-RR-4.md) |
| [US-RR-40-1](tasks/US-RR-40-1.md) | Test: Every table in every synthetic ROM decodes to the generator's manifest values (raw and real) via the Table API | ⚪ todo | — |  | — | — | [US-RR-40](stories/US-RR-40.md) |
| [US-RR-40-2](tasks/US-RR-40-2.md) | Test: Two tables overlapping the same bytes with different storage widths always reflect the current image after any ... | ⚪ todo | — |  | — | — | [US-RR-40](stories/US-RR-40.md) |
| [US-RR-40-3](tasks/US-RR-40-3.md) | Test: 3D tables report correct dimensions and compare/copy over all cells - no dummy 1x1 arrays (MF-12) | ⚪ todo | — |  | — | — | [US-RR-40](stories/US-RR-40.md) |
| [US-RR-40-4](tasks/US-RR-40-4.md) | Test: A table whose address range exceeds the image is disabled with an Error diagnostic and never panics (EC-02); ed... | ⚪ todo | — |  | — | — | [US-RR-40](stories/US-RR-40.md) |
| [US-RR-40-5](tasks/US-RR-40-5.md) | Test: Every table in every non-suspect corpus ROM with a definition decodes without Error diagnostics (results snapshot) | ⚪ todo | — |  | — | — | [US-RR-40](stories/US-RR-40.md) |
| [US-RR-41-1](tasks/US-RR-41-1.md) | Test: On a VirtualClock the engine never sends a request earlier than P3min after the last response, and applies each... | ⚪ todo | — |  | — | — | [US-RR-41](stories/US-RR-41.md) |
| [US-RR-41-2](tasks/US-RR-41-2.md) | Test: A corrupted or truncated response yields None values and a link-health event; a test proves the previous good r... | ⚪ todo | — |  | — | — | [US-RR-41](stories/US-RR-41.md) |
| [US-RR-41-3](tasks/US-RR-41-3.md) | Test: Timeouts are wall-clock deadlines from Clock, independent of how often the link is polled (MF-27) | ⚪ todo | — |  | — | — | [US-RR-41](stories/US-RR-41.md) |
| [US-RR-41-4](tasks/US-RR-41-4.md) | Test: ECU going silent mid-session triggers the recovery policy then a clean disconnect with all values None (EC-15);... | ⚪ todo | — |  | — | — | [US-RR-41](stories/US-RR-41.md) |
| [US-RR-41-5](tasks/US-RR-41-5.md) | Test: An unknown ECU ID runs with base-profile layers only and emits a Warning (EC-20) | ⚪ todo | — |  | — | — | [US-RR-41](stories/US-RR-41.md) |
| [US-RR-41-6](tasks/US-RR-41-6.md) | Test: cargo-mutants reports zero surviving mutants in timing enforcement and response validation | ⚪ todo | — |  | — | — | [US-RR-41](stories/US-RR-41.md) |
| [US-RR-42-1](tasks/US-RR-42-1.md) | Test: A CI check fails if any release binary requires a GLIBC symbol newer than 2.28 | ⚪ todo | — |  | — | — | [US-RR-42](stories/US-RR-42.md) |
| [US-RR-42-2](tasks/US-RR-42-2.md) | Test: romraider-cli --version runs in Ubuntu 20.04 and Rocky 8 containers (x86_64) and in an arm64 Debian container u... | ⚪ todo | — |  | — | — | [US-RR-42](stories/US-RR-42.md) |
| [US-RR-42-3](tasks/US-RR-42-3.md) | Test: ldd on release binaries shows no libudev dependency and port enumeration works via sysfs (test on Linux CI) | ⚪ todo | — |  | — | — | [US-RR-42](stories/US-RR-42.md) |
| [US-RR-43-1](tasks/US-RR-43-1.md) | Test: The .rpm installs and romraider-cli --version runs in clean Fedora and Rocky 8 containers | ⚪ todo | — |  | — | — | [US-RR-43](stories/US-RR-43.md) |
| [US-RR-43-2](tasks/US-RR-43-2.md) | Test: The PKGBUILD builds and installs in a clean Arch container (makepkg) and namcap reports no errors | ⚪ todo | — |  | — | — | [US-RR-43](stories/US-RR-43.md) |
| [US-RR-43-3](tasks/US-RR-43-3.md) | Test: install.sh user-local install and uninstall leave no files behind (test diffs the filesystem before and after);... | ⚪ todo | — |  | — | — | [US-RR-43](stories/US-RR-43.md) |
| [US-RR-44-1](tasks/US-RR-44-1.md) | Test: Property test: an operation that fails for any selected cell changes zero bytes (atomic all-or-nothing, T6) | ⚪ todo | — |  | — | — | [US-RR-44](stories/US-RR-44.md) |
| [US-RR-44-2](tasks/US-RR-44-2.md) | Test: Interpolation across a selection with a non-linear scaling produces linearly spaced real values (within one raw... | ⚪ todo | — |  | — | — | [US-RR-44](stories/US-RR-44.md) |
| [US-RR-44-3](tasks/US-RR-44-3.md) | Test: Increment by step k from value v yields exactly v+k in real space or a rejection - never a doubled or retried s... | ⚪ todo | — |  | — | — | [US-RR-44](stories/US-RR-44.md) |
| [US-RR-44-4](tasks/US-RR-44-4.md) | Test: Multiply and set are locale-independent: no string round-trip anywhere in the operation path (MF-39) | ⚪ todo | — |  | — | — | [US-RR-44](stories/US-RR-44.md) |
| [US-RR-44-5](tasks/US-RR-44-5.md) | Test: cargo-mutants reports zero surviving mutants in the operation and write path | ⚪ todo | — |  | — | — | [US-RR-44](stories/US-RR-44.md) |
| [US-RR-45-1](tasks/US-RR-45-1.md) | Test: Saving with a failing checksum and no override token is refused and the destination file is untouched (T6) | ⚪ todo | — |  | — | — | [US-RR-45](stories/US-RR-45.md) |
| [US-RR-45-2](tasks/US-RR-45-2.md) | Test: A simulated crash at every step of the save (fault injection between write, fsync and rename) leaves either the... | ⚪ todo | — |  | — | — | [US-RR-45](stories/US-RR-45.md) |
| [US-RR-45-3](tasks/US-RR-45-3.md) | Test: Save to a read-only directory, a path with spaces, a unicode path and a path on a different filesystem than tem... | ⚪ todo | — |  | — | — | [US-RR-45](stories/US-RR-45.md) |
| [US-RR-45-4](tasks/US-RR-45-4.md) | Test: Every saved file re-opens and validates with zero checksum failures (round-trip test across the corpus with ran... | ⚪ todo | — |  | — | — | [US-RR-45](stories/US-RR-45.md) |
| [US-RR-45-5](tasks/US-RR-45-5.md) | Test: An override save records who/when/which checksums failed in the sidecar journal | ⚪ todo | — |  | — | — | [US-RR-45](stories/US-RR-45.md) |
| [US-RR-46-1](tasks/US-RR-46-1.md) | Test: Property test: applying diff(a,b) to a yields b exactly, and diff(a,a) is empty | ⚪ todo | — |  | — | — | [US-RR-46](stories/US-RR-46.md) |
| [US-RR-46-2](tasks/US-RR-46-2.md) | Test: Diff reports raw and real differences per table and is linear time (benchmark on 1 MB images) | ⚪ todo | — |  | — | — | [US-RR-46](stories/US-RR-46.md) |
| [US-RR-46-3](tasks/US-RR-46-3.md) | Test: Clipboard copy then paste round-trips every table type exactly, under any process locale (test runs with de_DE ... | ⚪ todo | — |  | — | — | [US-RR-46](stories/US-RR-46.md) |
| [US-RR-46-4](tasks/US-RR-46-4.md) | Test: Pasting legacy-format clipboard text produces the same values as the legacy catalogue fixture specifies | ⚪ todo | — |  | — | — | [US-RR-46](stories/US-RR-46.md) |
| [US-RR-47-1](tasks/US-RR-47-1.md) | Test: Request builders produce byte-exact frames for init, read-address, read-memory, write-address and write-memory ... | ⚪ todo | — |  | — | — | [US-RR-47](stories/US-RR-47.md) |
| [US-RR-47-2](tasks/US-RR-47-2.md) | Test: Response parser rejects bad header, length or checksum with a diagnostic, and the fuzz target finds no panics | ⚪ todo | — |  | — | — | [US-RR-47](stories/US-RR-47.md) |
| [US-RR-47-3](tasks/US-RR-47-3.md) | Test: Requesting more addresses than the per-request limit splits into multiple requests and reassembles values in or... | ⚪ todo | — |  | — | — | [US-RR-47](stories/US-RR-47.md) |
| [US-RR-47-4](tasks/US-RR-47-4.md) | Test: Two concurrent simulated sessions with different ECUs do not interfere (no shared codec state, MF-29) | ⚪ todo | — |  | — | — | [US-RR-47](stories/US-RR-47.md) |
| [US-RR-47-5](tasks/US-RR-47-5.md) | Test: A full simulated logging loop (init, identify, poll 20 params) passes against the simulator with and without fa... | ⚪ todo | — |  | — | — | [US-RR-47](stories/US-RR-47.md) |
| [US-RR-48-1](tasks/US-RR-48-1.md) | Test: All 7 distro containers pass install, version, doctor, fixture decode and uninstall in CI | ⚪ todo | — |  | — | — | [US-RR-48](stories/US-RR-48.md) |
| [US-RR-48-2](tasks/US-RR-48-2.md) | Test: A deliberately broken package (missing binary) fails the release workflow before publishing | ⚪ todo | — |  | — | — | [US-RR-48](stories/US-RR-48.md) |
| [US-RR-48-3](tasks/US-RR-48-3.md) | Test: Uninstall leaves no files outside user data directories | ⚪ todo | — |  | — | — | [US-RR-48](stories/US-RR-48.md) |
| [US-RR-49-1](tasks/US-RR-49-1.md) | Test: x86_64 and i686 MSIs install, run romraider-cli --version and uninstall cleanly on a Windows CI runner | ⚪ todo | — |  | — | — | [US-RR-49](stories/US-RR-49.md) |
| [US-RR-49-2](tasks/US-RR-49-2.md) | Test: The i686 binary is a genuine 32-bit PE (checked in CI) | ⚪ todo | — |  | — | — | [US-RR-49](stories/US-RR-49.md) |
| [US-RR-49-3](tasks/US-RR-49-3.md) | Test: Release notes state signing status explicitly | ⚪ todo | — |  | — | — | [US-RR-49](stories/US-RR-49.md) |
| [US-RR-5-1](tasks/US-RR-5-1.md) | Test: A template fuzz target runs locally with cargo +nightly fuzz run and in the nightly workflow | ⚪ todo | — |  | — | — | [US-RR-5](stories/US-RR-5.md) |
| [US-RR-5-2](tasks/US-RR-5-2.md) | Test: A planted panic in a toy target is found by the nightly job and auto-filed as an issue | ⚪ todo | — |  | — | — | [US-RR-5](stories/US-RR-5.md) |
| [US-RR-5-3](tasks/US-RR-5-3.md) | Test: Committed crash inputs run as ordinary regression tests in ci.yml without nightly | ⚪ todo | — |  | — | — | [US-RR-5](stories/US-RR-5.md) |
| [US-RR-5-4](tasks/US-RR-5-4.md) | Test: Fuzz targets enforce a per-input time and memory limit so hangs and unbounded allocation count as failures | ⚪ todo | — |  | — | — | [US-RR-5](stories/US-RR-5.md) |
| [US-RR-50-1](tasks/US-RR-50-1.md) | Test: verify exits 0 on every non-suspect corpus ROM and non-zero with a per-checksum report on a corrupted copy | ⚪ todo | — |  | — | — | [US-RR-50](stories/US-RR-50.md) |
| [US-RR-50-2](tasks/US-RR-50-2.md) | Test: edit without the explicit write confirmation flag refuses to write; edit never overwrites the input path unless... | ⚪ todo | — |  | — | — | [US-RR-50](stories/US-RR-50.md) |
| [US-RR-50-3](tasks/US-RR-50-3.md) | Test: decode output for synthetic ROMs matches the generator manifest exactly (integration test) | ⚪ todo | — |  | — | — | [US-RR-50](stories/US-RR-50.md) |
| [US-RR-50-4](tasks/US-RR-50-4.md) | Test: All commands return structured JSON errors with diagnostic codes when --json is set (snapshot tests) | ⚪ todo | — |  | — | — | [US-RR-50](stories/US-RR-50.md) |
| [US-RR-51-1](tasks/US-RR-51-1.md) | Test: The suite runs in CI over the cached external corpus and fails on any regression in the committed coverage report | ⚪ todo | — |  | — | — | [US-RR-51](stories/US-RR-51.md) |
| [US-RR-51-2](tasks/US-RR-51-2.md) | Test: Every non-suspect corpus ROM with a definition passes all listed invariants; any exception is an explicit, reas... | ⚪ todo | — |  | — | — | [US-RR-51](stories/US-RR-51.md) |
| [US-RR-51-3](tasks/US-RR-51-3.md) | Test: The two non-standard-size images are classified (e.g. CRLF-mangled vs trailing data) with evidence, not just sk... | ⚪ todo | — |  | — | — | [US-RR-51](stories/US-RR-51.md) |
| [US-RR-51-4](tasks/US-RR-51-4.md) | Test: Randomised edit-save-reopen runs at least 1000 iterations across the corpus per CI run with a fixed seed record... | ⚪ todo | — |  | — | — | [US-RR-51](stories/US-RR-51.md) |
| [US-RR-52-1](tasks/US-RR-52-1.md) | Test: Across the full simulator test suite (all protocols, all faults), the simulator records zero write opcodes with... | ⚪ todo | — |  | — | — | [US-RR-52](stories/US-RR-52.md) |
| [US-RR-52-2](tasks/US-RR-52-2.md) | Test: A write with a token scoped to a different ECU ID or address range is refused before anything is sent | ⚪ todo | — |  | — | — | [US-RR-52](stories/US-RR-52.md) |
| [US-RR-52-3](tasks/US-RR-52-3.md) | Test: A test fails if any function other than the gated write path can construct a frame containing write opcodes (0x... | ⚪ todo | — |  | — | — | [US-RR-52](stories/US-RR-52.md) |
| [US-RR-52-4](tasks/US-RR-52-4.md) | Test: cargo-mutants reports zero surviving mutants in the write-gating module | ⚪ todo | — |  | — | — | [US-RR-52](stories/US-RR-52.md) |
| [US-RR-53-1](tasks/US-RR-53-1.md) | Test: The rules file is generated from adapter profiles and a CI check fails if the committed file is out of date | ⚪ todo | — |  | — | — | [US-RR-53](stories/US-RR-53.md) |
| [US-RR-53-2](tasks/US-RR-53-2.md) | Test: doctor detects each condition (EACCES, brltty, ModemManager, missing device, high latency timer) in tests using... | ⚪ todo | — |  | — | — | [US-RR-53](stories/US-RR-53.md) |
| [US-RR-53-3](tasks/US-RR-53-3.md) | Test: On a fresh Ubuntu 24.04 install with the .deb, the KKL cable opens without any manual permission change (HIL re... | ⚪ todo | — |  | — | — | [US-RR-53](stories/US-RR-53.md) |
| [US-RR-53-4](tasks/US-RR-53-4.md) | Test: The pkexec install path displays the rule file contents before asking for elevation | ⚪ todo | — |  | — | — | [US-RR-53](stories/US-RR-53.md) |
| [US-RR-54-1](tasks/US-RR-54-1.md) | Test: A parameter missing from a response appears as None in the frame and never as 0.0 (MF-09) - replay and simulato... | ⚪ todo | — |  | — | — | [US-RR-54](stories/US-RR-54.md) |
| [US-RR-54-2](tasks/US-RR-54-2.md) | Test: A stalled subscriber never blocks the producer; dropped-frame counters are exposed and tested | ⚪ todo | — |  | — | — | [US-RR-54](stories/US-RR-54.md) |
| [US-RR-54-3](tasks/US-RR-54-3.md) | Test: Start/stop under load 1000 times leaves no threads or channels alive (stress test, MF-34) | ⚪ todo | — |  | — | — | [US-RR-54](stories/US-RR-54.md) |
| [US-RR-55-1](tasks/US-RR-55-1.md) | Test: Property test: for any acyclic graph, evaluation order respects dependencies and results match direct evaluation | ⚪ todo | — |  | — | — | [US-RR-55](stories/US-RR-55.md) |
| [US-RR-55-2](tasks/US-RR-55-2.md) | Test: A None or stale input yields None for every downstream derived signal, never 0 (EC-25) | ⚪ todo | — |  | — | — | [US-RR-55](stories/US-RR-55.md) |
| [US-RR-55-3](tasks/US-RR-55-3.md) | Test: A derived signal depending on a wideband device signal works end to end in a simulator test | ⚪ todo | — |  | — | — | [US-RR-55](stories/US-RR-55.md) |
| [US-RR-55-4](tasks/US-RR-55-4.md) | Test: Cycles produce a diagnostic naming the cycle at load time | ⚪ todo | — |  | — | — | [US-RR-55](stories/US-RR-55.md) |
| [US-RR-56-1](tasks/US-RR-56-1.md) | Test: Each profiled device decodes a replay fixture (built from the legacy mock data and protocol docs) to hand-check... | ⚪ todo | — |  | — | — | [US-RR-56](stories/US-RR-56.md) |
| [US-RR-56-2](tasks/US-RR-56-2.md) | Test: A device frame failing its checksum is dropped and counted, never decoded (MF-10) | ⚪ todo | — |  | — | — | [US-RR-56](stories/US-RR-56.md) |
| [US-RR-56-3](tasks/US-RR-56-3.md) | Test: Values older than the TTL become None; unplugging a device mid-session yields None within TTL (EC-24, MF-26) | ⚪ todo | — |  | — | — | [US-RR-56](stories/US-RR-56.md) |
| [US-RR-56-4](tasks/US-RR-56-4.md) | Test: Selecting a device signal in a view does not open the port; only the sampling engine does (MF-32); renaming a d... | ⚪ todo | — |  | — | — | [US-RR-56](stories/US-RR-56.md) |
| [US-RR-56-5](tasks/US-RR-56-5.md) | Test: Every device frame parser is fuzzed with no panics | ⚪ todo | — |  | — | — | [US-RR-56](stories/US-RR-56.md) |
| [US-RR-57-1](tasks/US-RR-57-1.md) | Test: Property test: frames -> CSV -> playback reproduces identical frames (values, None gaps and timestamps) | ⚪ todo | — |  | — | — | [US-RR-57](stories/US-RR-57.md) |
| [US-RR-57-2](tasks/US-RR-57-2.md) | Test: A parameter-set change mid-session rotates to a new file and the header is never rewritten mid-file (MF-30, EC-27) | ⚪ todo | — |  | — | — | [US-RR-57](stories/US-RR-57.md) |
| [US-RR-57-3](tasks/US-RR-57-3.md) | Test: Output is byte-identical under de_DE, fr_FR and en_AU locales | ⚪ todo | — |  | — | — | [US-RR-57](stories/US-RR-57.md) |
| [US-RR-57-4](tasks/US-RR-57-4.md) | Test: Importer KAT fixtures for legacy RomRaider, Cobb, AEM and OP2 headers, including comma-decimal legacy files (EC... | ⚪ todo | — |  | — | — | [US-RR-57](stories/US-RR-57.md) |
| [US-RR-57-5](tasks/US-RR-57-5.md) | Test: A wall-clock jump during logging keeps the monotonic column strictly increasing (EC-26) | ⚪ todo | — |  | — | — | [US-RR-57](stories/US-RR-57.md) |
| [US-RR-58-1](tasks/US-RR-58-1.md) | Test: No collector references a protocol-specific parameter ID (grep test plus a role-binding unit test per collector) | ⚪ todo | — |  | — | — | [US-RR-58](stories/US-RR-58.md) |
| [US-RR-58-2](tasks/US-RR-58-2.md) | Test: Dyno power and torque match hand-calculated KAT fixtures for at least 3 vehicle specs with sources | ⚪ todo | — |  | — | — | [US-RR-58](stories/US-RR-58.md) |
| [US-RR-58-3](tasks/US-RR-58-3.md) | Test: MAF and injector collectors produce identical results from an SSM replay and an equivalent synthetic OBD replay... | ⚪ todo | — |  | — | — | [US-RR-58](stories/US-RR-58.md) |
| [US-RR-58-4](tasks/US-RR-58-4.md) | Test: FilterSet rejections are reported per filter with counts | ⚪ todo | — |  | — | — | [US-RR-58](stories/US-RR-58.md) |
| [US-RR-59-1](tasks/US-RR-59-1.md) | Test: Killing the process at any point during a settings save leaves the previous settings intact (fault-injection te... | ⚪ todo | — |  | — | — | [US-RR-59](stories/US-RR-59.md) |
| [US-RR-59-2](tasks/US-RR-59-2.md) | Test: A second launch with a path containing spaces and unicode opens it in the running instance (MF-36); the IPC end... | ⚪ todo | — |  | — | — | [US-RR-59](stories/US-RR-59.md) |
| [US-RR-59-3](tasks/US-RR-59-3.md) | Test: Java settings.xml import produces the expected typed settings for a hand-checked fixture and never modifies the... | ⚪ todo | — |  | — | — | [US-RR-59](stories/US-RR-59.md) |
| [US-RR-59-4](tasks/US-RR-59-4.md) | Test: No settings file in the current working directory is ever read implicitly (MF-35) | ⚪ todo | — |  | — | — | [US-RR-59](stories/US-RR-59.md) |
| [US-RR-6-1](tasks/US-RR-6-1.md) | Test: Tests requiring the external or private corpus are reported as skipped (with the env var to set) when it is abs... | ⚪ todo | — |  | — | — | [US-RR-6](stories/US-RR-6.md) |
| [US-RR-6-2](tasks/US-RR-6-2.md) | Test: The fixture loader rejects a fixture file with an unknown schema version | ⚪ todo | — |  | — | — | [US-RR-6](stories/US-RR-6.md) |
| [US-RR-6-3](tasks/US-RR-6-3.md) | Test: rr-testkit cannot be a normal (non-dev) dependency of any crate - enforced by the dependency-rule test | ⚪ todo | — |  | — | — | [US-RR-6](stories/US-RR-6.md) |
| [US-RR-6-4](tasks/US-RR-6-4.md) | Test: VirtualClock helpers let a test advance time deterministically with no real sleeping (test runs in under 10 ms ... | ⚪ todo | — |  | — | — | [US-RR-6](stories/US-RR-6.md) |
| [US-RR-60-1](tasks/US-RR-60-1.md) | Test: log and identify against the simulator produce CSV and ID output matching the simulator's configured ECU (integ... | ⚪ todo | — |  | — | — | [US-RR-60](stories/US-RR-60.md) |
| [US-RR-60-2](tasks/US-RR-60-2.md) | Test: replay of a captured simulator trace reproduces the same CSV byte-for-byte | ⚪ todo | — |  | — | — | [US-RR-60](stories/US-RR-60.md) |
| [US-RR-60-3](tasks/US-RR-60-3.md) | Test: scrub removes every VIN and serial field from a trace and log (planted-value test) and the result still replays | ⚪ todo | — |  | — | — | [US-RR-60](stories/US-RR-60.md) |
| [US-RR-60-4](tasks/US-RR-60-4.md) | Test: No CLI command can issue an ECU write; log, capture and identify are read-only by construction (T6) | ⚪ todo | — |  | — | — | [US-RR-60](stories/US-RR-60.md) |
| [US-RR-61-1](tasks/US-RR-61-1.md) | Test: Every table kind, including bitwise switches, renders in the widget (UI smoke test per kind, MF-38) | ⚪ todo | — |  | — | — | [US-RR-61](stories/US-RR-61.md) |
| [US-RR-61-2](tasks/US-RR-61-2.md) | Test: Edit then undo then redo in the UI produces the same bytes as the equivalent core operations (egui_kittest) | ⚪ todo | — |  | — | — | [US-RR-61](stories/US-RR-61.md) |
| [US-RR-61-3](tasks/US-RR-61-3.md) | Test: A rejected edit shows per-cell reasons and leaves the ROM unchanged | ⚪ todo | — |  | — | — | [US-RR-61](stories/US-RR-61.md) |
| [US-RR-61-4](tasks/US-RR-61-4.md) | Test: A 1 MB ROM with 2000 tables opens and the tree is interactive within 1 second on CI hardware | ⚪ todo | — |  | — | — | [US-RR-61](stories/US-RR-61.md) |
| [US-RR-62-1](tasks/US-RR-62-1.md) | Test: With a failing checksum, the default save path does not write; only the explicit typed override does (egui_kitt... | ⚪ todo | — |  | — | — | [US-RR-62](stories/US-RR-62.md) |
| [US-RR-62-2](tasks/US-RR-62-2.md) | Test: A test enumerates RomWrite token minting sites and fails if any exist outside the confirmation flow module | ⚪ todo | — |  | — | — | [US-RR-62](stories/US-RR-62.md) |
| [US-RR-62-3](tasks/US-RR-62-3.md) | Test: Overwriting the original source file requires a separate explicit confirmation and a .bak is created | ⚪ todo | — |  | — | — | [US-RR-62](stories/US-RR-62.md) |
| [US-RR-62-4](tasks/US-RR-62-4.md) | Test: Override saves are visible in the journal viewer with which checksums failed | ⚪ todo | — |  | — | — | [US-RR-62](stories/US-RR-62.md) |
| [US-RR-63-1](tasks/US-RR-63-1.md) | Test: Driven by the simulator with a dropped-parameter fault, the affected gauge shows a missing state, not 0 (egui_k... | ⚪ todo | — |  | — | — | [US-RR-63](stories/US-RR-63.md) |
| [US-RR-63-2](tasks/US-RR-63-2.md) | Test: Link-health indicator changes state within one poll cycle of a simulated disconnect | ⚪ todo | — |  | — | — | [US-RR-63](stories/US-RR-63.md) |
| [US-RR-63-3](tasks/US-RR-63-3.md) | Test: Warning thresholds persist in the user profile and round-trip (MF-41) | ⚪ todo | — |  | — | — | [US-RR-63](stories/US-RR-63.md) |
| [US-RR-63-4](tasks/US-RR-63-4.md) | Test: The UI thread never blocks on the link: a stalled simulator does not freeze rendering (frame-time assertion) | ⚪ todo | — |  | — | — | [US-RR-63](stories/US-RR-63.md) |
| [US-RR-64-1](tasks/US-RR-64-1.md) | Test: The Liberty's ECU ID and ROM ID are recorded in the HIL report and in VISION.md (replacing TBD) | ⚪ todo | — |  | — | — | [US-RR-64](stories/US-RR-64.md) |
| [US-RR-64-2](tasks/US-RR-64-2.md) | Test: Each scenario (idle, drive, unplug/replug, ignition off, engine start) has a scrubbed trace committed as a repl... | ⚪ todo | — |  | — | — | [US-RR-64](stories/US-RR-64.md) |
| [US-RR-64-3](tasks/US-RR-64-3.md) | Test: The HIL run's simulator-trace equivalents expose no behaviour the simulator did not model; any gap becomes a ne... | ⚪ todo | — |  | — | — | [US-RR-64](stories/US-RR-64.md) |
| [US-RR-64-4](tasks/US-RR-64-4.md) | Test: The HIL harness refuses to run any write operation (asserted in its own test) | ⚪ todo | — |  | — | — | [US-RR-64](stories/US-RR-64.md) |
| [US-RR-65-1](tasks/US-RR-65-1.md) | Test: A spike report documents which address ranges respond to A0, the working block size and measured throughput, wi... | ⚪ todo | — |  | — | — | [US-RR-65](stories/US-RR-65.md) |
| [US-RR-65-2](tasks/US-RR-65-2.md) | Test: If feasible: two full reads produce identical SHA-256 and the image validates under its checksum spec, and it i... | ⚪ todo | — |  | — | — | [US-RR-65](stories/US-RR-65.md) |
| [US-RR-65-3](tasks/US-RR-65-3.md) | Test: The spike code issues no write opcodes (verified by its trace) | ⚪ todo | — |  | — | — | [US-RR-65](stories/US-RR-65.md) |
| [US-RR-65-4](tasks/US-RR-65-4.md) | Test: Outcome and next step recorded as a DECISIONS entry | ⚪ todo | — |  | — | — | [US-RR-65](stories/US-RR-65.md) |
| [US-RR-66-1](tasks/US-RR-66-1.md) | Test: Selecting cells in the 3D view selects the same cells in the grid and vice versa (UI test) | ⚪ todo | — |  | — | — | [US-RR-66](stories/US-RR-66.md) |
| [US-RR-66-2](tasks/US-RR-66-2.md) | Test: Renders under both X11 and Wayland with the glow renderer in CI (headless smoke) | ⚪ todo | — |  | — | — | [US-RR-66](stories/US-RR-66.md) |
| [US-RR-66-3](tasks/US-RR-66-3.md) | Test: Rendering choice recorded as a DECISIONS entry | ⚪ todo | — |  | — | — | [US-RR-66](stories/US-RR-66.md) |
| [US-RR-67-1](tasks/US-RR-67-1.md) | Test: Replaying a trace highlights the correct table cells for each frame (deterministic UI snapshot at chosen timest... | ⚪ todo | — |  | — | — | [US-RR-67](stories/US-RR-67.md) |
| [US-RR-67-2](tasks/US-RR-67-2.md) | Test: Learning-table values from a replay fixture display through the standard Table widget with no separate panel code | ⚪ todo | — |  | — | — | [US-RR-67](stories/US-RR-67.md) |
| [US-RR-67-3](tasks/US-RR-67-3.md) | Test: Overlay goes inactive when the frame is stale or the link is down | ⚪ todo | — |  | — | — | [US-RR-67](stories/US-RR-67.md) |
| [US-RR-68-1](tasks/US-RR-68-1.md) | Test: All listed flows run headless in CI on Linux and Windows | ⚪ todo | — |  | — | — | [US-RR-68](stories/US-RR-68.md) |
| [US-RR-68-2](tasks/US-RR-68-2.md) | Test: The refused failing-checksum save flow is part of the release-blocking T6 job | ⚪ todo | — |  | — | — | [US-RR-68](stories/US-RR-68.md) |
| [US-RR-68-3](tasks/US-RR-68-3.md) | Test: Snapshot diffs are reviewable artifacts on failure | ⚪ todo | — |  | — | — | [US-RR-68](stories/US-RR-68.md) |
| [US-RR-69-1](tasks/US-RR-69-1.md) | Test: A mock PassThru shared library built in-tree exercises connect, ioctl SET_CONFIG, read/write msgs and error pat... | ⚪ todo | — |  | — | — | [US-RR-69](stories/US-RR-69.md) |
| [US-RR-69-2](tasks/US-RR-69-2.md) | Test: A J2534 library placed in the current working directory is never loaded (test) | ⚪ todo | — |  | — | — | [US-RR-69](stories/US-RR-69.md) |
| [US-RR-69-3](tasks/US-RR-69-3.md) | Test: Loading a 32-bit DLL from a 64-bit process produces a specific diagnostic recommending the i686 build | ⚪ todo | — |  | — | — | [US-RR-69](stories/US-RR-69.md) |
| [US-RR-69-4](tasks/US-RR-69-4.md) | Test: No unsafe block outside the FFI module, and every unsafe block has a SAFETY comment (lint) | ⚪ todo | — |  | — | — | [US-RR-69](stories/US-RR-69.md) |
| [US-RR-7-1](tasks/US-RR-7-1.md) | Test: trybuild compile-fail tests prove a token cannot be constructed, cloned into existence, defaulted, deserialized... | ⚪ todo | — |  | — | — | [US-RR-7](stories/US-RR-7.md) |
| [US-RR-7-2](tasks/US-RR-7-2.md) | Test: A test enumerates all public functions in rr-rom and rr-comms that perform writes (via a marker trait or attrib... | ⚪ todo | — |  | — | — | [US-RR-7](stories/US-RR-7.md) |
| [US-RR-7-3](tasks/US-RR-7-3.md) | Test: Tokens are scoped: an EcuMemoryWrite token carries the ECU ID and address range it was granted for, and use out... | ⚪ todo | — |  | — | — | [US-RR-7](stories/US-RR-7.md) |
| [US-RR-7-4](tasks/US-RR-7-4.md) | Test: The T6 suite runs in ci.yml as a release-blocking job | ⚪ todo | — |  | — | — | [US-RR-7](stories/US-RR-7.md) |
| [US-RR-70-1](tasks/US-RR-70-1.md) | Test: Byte-exact request KATs for each service, and response parsing fuzzed | ⚪ todo | — |  | — | — | [US-RR-70](stories/US-RR-70.md) |
| [US-RR-70-2](tasks/US-RR-70-2.md) | Test: The simulator responder passes the full fault matrix | ⚪ todo | — |  | — | — | [US-RR-70](stories/US-RR-70.md) |
| [US-RR-70-3](tasks/US-RR-70-3.md) | Test: Default timings come from the standard-vector fixtures with no literals in codec code | ⚪ todo | — |  | — | — | [US-RR-70](stories/US-RR-70.md) |
| [US-RR-71-1](tasks/US-RR-71-1.md) | Test: The AT initialisation sequence is generated from the profile with no hard-coded command strings in Link code | ⚪ todo | — |  | — | — | [US-RR-71](stories/US-RR-71.md) |
| [US-RR-71-2](tasks/US-RR-71-2.md) | Test: An ELM simulator covers OK/ERROR/NO DATA/BUS INIT failure responses and clone quirk rules | ⚪ todo | — |  | — | — | [US-RR-71](stories/US-RR-71.md) |
| [US-RR-71-3](tasks/US-RR-71-3.md) | Test: Prompt framing handles partial and multi-line responses (property test) | ⚪ todo | — |  | — | — | [US-RR-71](stories/US-RR-71.md) |
| [US-RR-72-1](tasks/US-RR-72-1.md) | Test: Byte-exact KATs for each DS2 command type from the legacy catalogue | ⚪ todo | — |  | — | — | [US-RR-72](stories/US-RR-72.md) |
| [US-RR-72-2](tasks/US-RR-72-2.md) | Test: A baud change mid-session works against the simulator, including a fault during the switch (EC-22) | ⚪ todo | — |  | — | — | [US-RR-72](stories/US-RR-72.md) |
| [US-RR-72-3](tasks/US-RR-72-3.md) | Test: DS2 learning tables render through the generic Table type with no DS2-specific view code | ⚪ todo | — |  | — | — | [US-RR-72](stories/US-RR-72.md) |
| [US-RR-73-1](tasks/US-RR-73-1.md) | Test: For at least 3 corpus ROMs with both RomRaider XML and XDF definitions, the cross-source harness reports zero d... | ⚪ todo | — |  | — | — | [US-RR-73](stories/US-RR-73.md) |
| [US-RR-73-2](tasks/US-RR-73-2.md) | Test: Scale factors between 0.99 and 1.01 are preserved exactly (MF-20); float XDF tables import (MF-21) | ⚪ todo | — |  | — | — | [US-RR-73](stories/US-RR-73.md) |
| [US-RR-73-3](tasks/US-RR-73-3.md) | Test: XDF/VDF parsing is fuzzed with no panics | ⚪ todo | — |  | — | — | [US-RR-73](stories/US-RR-73.md) |
| [US-RR-74-1](tasks/US-RR-74-1.md) | Test: The licence compatibility finding is recorded in DECISIONS.md before any reference code is read | ⚪ todo | — |  | — | — | [US-RR-74](stories/US-RR-74.md) |
| [US-RR-74-2](tasks/US-RR-74-2.md) | Test: If pursued: SSM logging over native OP2 produces traces equivalent to the J2534 path on the same car (HIL report) | ⚪ todo | — |  | — | — | [US-RR-74](stories/US-RR-74.md) |
| [US-RR-74-3](tasks/US-RR-74-3.md) | Test: The spike issues no write operations, and outcome and recommendation are recorded as a DECISIONS update to D-023 | ⚪ todo | — |  | — | — | [US-RR-74](stories/US-RR-74.md) |
| [US-RR-75-1](tasks/US-RR-75-1.md) | Test: For at least 3 corpus ROMs, ECUFlash vs RomRaider XML cross-source agreement reports zero disagreements on matc... | ⚪ todo | — |  | — | — | [US-RR-75](stories/US-RR-75.md) |
| [US-RR-75-2](tasks/US-RR-75-2.md) | Test: ECUFlash include/inheritance resolves through the shared Resolver with cycle diagnostics | ⚪ todo | — |  | — | — | [US-RR-75](stories/US-RR-75.md) |
| [US-RR-75-3](tasks/US-RR-75-3.md) | Test: ECUFlash XML parsing is fuzzed with no panics | ⚪ todo | — |  | — | — | [US-RR-75](stories/US-RR-75.md) |
| [US-RR-76-1](tasks/US-RR-76-1.md) | Test: The SSM CAN codec shares the K-line codec's message semantics (no duplicated parsing logic - review and test re... | ⚪ todo | — |  | — | — | [US-RR-76](stories/US-RR-76.md) |
| [US-RR-76-2](tasks/US-RR-76-2.md) | Test: ISO-TP multi-frame responses reassemble correctly under split/out-of-order faults in the simulator | ⚪ todo | — |  | — | — | [US-RR-76](stories/US-RR-76.md) |
| [US-RR-76-3](tasks/US-RR-76-3.md) | Test: Byte-exact KATs for CAN request/response variants | ⚪ todo | — |  | — | — | [US-RR-76](stories/US-RR-76.md) |
| [US-RR-77-1](tasks/US-RR-77-1.md) | Test: Every implemented PID decodes the J1979 vector fixtures exactly, including boundary values | ⚪ todo | — |  | — | — | [US-RR-77](stories/US-RR-77.md) |
| [US-RR-77-2](tasks/US-RR-77-2.md) | Test: Supported-PID bitmap walking matches hand-checked fixtures across all ranges | ⚪ todo | — |  | — | — | [US-RR-77](stories/US-RR-77.md) |
| [US-RR-77-3](tasks/US-RR-77-3.md) | Test: DTC clear (mode 04) is only reachable with an EcuReset token (T6) | ⚪ todo | — |  | — | — | [US-RR-77](stories/US-RR-77.md) |
| [US-RR-78-1](tasks/US-RR-78-1.md) | Test: NCS codec vectors (transcribed from legacy tests) pass | ⚪ todo | — |  | — | — | [US-RR-78](stories/US-RR-78.md) |
| [US-RR-78-2](tasks/US-RR-78-2.md) | Test: Supported-PID discovery against the simulator matches the legacy catalogue's documented behaviour | ⚪ todo | — |  | — | — | [US-RR-78](stories/US-RR-78.md) |
| [US-RR-78-3](tasks/US-RR-78-3.md) | Test: No literal delays in NCS code - all waits come from the resolved profile (grep test plus trace timing assertion) | ⚪ todo | — |  | — | — | [US-RR-78](stories/US-RR-78.md) |
| [US-RR-8-1](tasks/US-RR-8-1.md) | Test: PR template contains a checklist covering test tiers touched, ledger entries closed with test names, concept ca... | ⚪ todo | — |  | — | — | [US-RR-8](stories/US-RR-8.md) |
| [US-RR-8-2](tasks/US-RR-8-2.md) | Test: CODEOWNERS requires a second approval for changes under the gated safety modules and branch protection enforces it | ⚪ todo | — |  | — | — | [US-RR-8](stories/US-RR-8.md) |
| [US-RR-8-3](tasks/US-RR-8-3.md) | Test: Issue template instructs users to run romraider-cli trace scrub before attaching traces | ⚪ todo | — |  | — | — | [US-RR-8](stories/US-RR-8.md) |
| [US-RR-8-4](tasks/US-RR-8-4.md) | Test: CONTRIBUTING.md states the Definition of Done and links PROJECT.md Testing Strategy and the ledgers | ⚪ todo | — |  | — | — | [US-RR-8](stories/US-RR-8.md) |
| [US-RR-9-1](tasks/US-RR-9-1.md) | Test: The validator rejects any expected value without a source field, and CI runs it on every fixture | ⚪ todo | — |  | — | — | [US-RR-9](stories/US-RR-9.md) |
| [US-RR-9-2](tasks/US-RR-9-2.md) | Test: A fixture with hand-calc source must include the working (formula and intermediate values) or it is rejected | ⚪ todo | — |  | — | — | [US-RR-9](stories/US-RR-9.md) |
| [US-RR-9-3](tasks/US-RR-9-3.md) | Test: At least 10 seed fixtures exist covering every storage type the format supports, each reviewed and sourced | ⚪ todo | — |  | — | — | [US-RR-9](stories/US-RR-9.md) |
| [US-RR-9-4](tasks/US-RR-9-4.md) | Test: Changing an expected value in an existing fixture requires a changelog line in the fixture explaining why (CI c... | ⚪ todo | — |  | — | — | [US-RR-9](stories/US-RR-9.md) |
