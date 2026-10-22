# Known-answer fixtures

This directory holds the public, in-repo fixture tier. Today that is the
known-answer (KAT) fixtures under [`kat/`](kat/): small, hand-written TOML
files that pin down what a run of bytes decodes to, or what a checksum is
before and after a fix. Each expected value says where it came from, and that
source is never the code under test or the Java application
([D-024](../.project/DECISIONS.md#d-024-no-java-in-the-project-including-tests-and-tooling)).

This README covers how to write a fixture, which sources are allowed, how
changes are reviewed, and how to run the validator. The fixture tiers
(public, `RR_CORPUS_DIR`, `RR_FIXTURES_DIR`) and how tests reach them are in
[CONTRIBUTING.md, Fixtures and corpora](../CONTRIBUTING.md#fixtures-and-corpora).
The project-wide rules are in
[CONTRIBUTING.md, Fixture source rule](../CONTRIBUTING.md#fixture-source-rule).

**Never commit a real ROM, or any part of one, in this directory.** Stock
ROMs belong in the external corpus (`RR_CORPUS_DIR`), and your own ROMs and
car traces in the private tier (`RR_FIXTURES_DIR`). A KAT fixture that needs
ROM bytes refers to them with `[input] rom = { tier = "corpus", ... }`
([below](#input)), and the bytes stay out of the repository. The ROM-leak
guard, `cargo xtask leak-guard` (story US-RR-13), rejects ROM images, ROM
fragments and unscrubbed VINs under `fixtures/public`, and stock-ROM bytes
anywhere in the repository. It runs in the `hooks/pre-commit` hook on what you
stage and in CI's `leak-guard` job on every tracked file; install the hook as
[CONTRIBUTING.md, ROM-leak guard](../CONTRIBUTING.md#rom-leak-guard) says.
Reviewers still check new binaries by hand. The only ROM-shaped
files in the repository are the generated synthetic ROMs in
[`public/synthetic/`](public/synthetic/), which are built from known values
and marked as synthetic ([Synthetic ROMs](#synthetic-roms)).

## Layout

| Path | What |
|------|------|
| [`fixtures/kat/*.toml`](kat/) | Seed KAT fixtures, one storage type or scaling per file, named `<storage>-<endian>.toml` (`u16-be.toml`, `i24-le.toml`, `u8-be.toml`), plus `bitfield-*`, `scaling-*` and `u20-movi20.toml`; and expression fixtures, `expr-*.toml` ([`expr-jep-precedence.toml`](kat/expr-jep-precedence.toml): JEP operator precedence and `^` semantics, US-RR-20-2; [`expr-corpus.toml`](kat/expr-corpus.toml): hand-checked formulas of the definition corpus, named by hash, US-RR-20-10; [`expr-bitwise-functions.toml`](kat/expr-bitwise-functions.toml): `BitWise`, `& \| << >>` and the standard functions, which the corpus does not use); and unit fixtures, `units-*.toml` ([`units-defined-factors.toml`](kat/units-defined-factors.toml): a cited known answer for every conversion factor of rr-core's unit registry, US-RR-21-1; [`units-afr-lambda.toml`](kat/units-afr-lambda.toml): air/fuel ratio ↔ lambda for each fuel's stoichiometric ratio, US-RR-21-5); and timing fixtures, `timing-<standard>.toml` (US-RR-14-5): the default timing parameters of ISO 9141-2:1994 (5-baud init W0–W4), ISO 14230-2:1999 (P1–P4, including P3min = 55 ms, MF-28), ISO 15765-2:2004 (N_As/N_Ar/N_Bs/N_Cr timeouts, STmin range) and ISO 15765-3:2004 (P2CAN_Server, P2*CAN_Server), each backing a constant of `rr_core::timing::iso`; and SAE J1979 service $01 PID vectors, `j1979-01-<signal>-<storage>.toml` (US-RR-14-6), ordinary `[[decode]]` fixtures described in [J1979 PID vectors](#j1979-pid-vectors); and checksum reference vectors, `checksum-*.toml` (US-RR-14-7): `[[checksum]]` fixtures for every sum, XOR and CRC variant the checksum engine plans, described in [Checksum reference vectors](#checksum-reference-vectors); and [`ncs-codec-legacy.toml`](kat/ncs-codec-legacy.toml) (US-RR-14-8): the NCS (Nissan) codec's encode, decode and CRC-16 vectors, a `[[codec]]` fixture transcribed as data from the legacy test file, described in [`[[codec]]` cases](#codec-cases). |
| [`defs-kat/romraider/*.toml`](defs-kat/romraider/) | Definition-reader KATs (US-RR-29-2): one per RomRaider table kind (`table-1d`, `table-2d`, `table-3d`, `table-switch`, `table-bitwise-switch`) and `inheritance` (`base` / `omit`), each a minimal ECU-definition XML and the typed document it must resolve to, written by hand. Not schema-1 byte KATs: see [Definition-reader fixtures](#definition-reader-fixtures). |
| [`crates/rr-testkit/tests/data/kat/*.toml`](../crates/rr-testkit/tests/data/kat/) | Samples for rr-testkit's own schema tests: a checksum fixture, a decode fixture with an inline conversion, and one that uses a ROM and definition reference. They are not seed fixtures, but the validator checks them too. |
| [`crates/rr-testkit/src/kat.rs`](../crates/rr-testkit/src/kat.rs) | The schema-1 types (serde model). Its module docs are the format reference. |
| [`crates/rr-testkit/src/kat/validate.rs`](../crates/rr-testkit/src/kat/validate.rs) | The validator rules. Its module docs are the reference for every rule. |
| [`public/ssm/*.toml`](public/ssm/) | SSM K-line simulator fixtures (US-RR-12-4): byte-exact exchanges written from [`docs/legacy/ssm.md`](../docs/legacy/ssm.md) for the ECU simulator (US-RR-36) and the SSM codec (US-RR-47). Synthetic, text only. See [SSM simulator fixtures](#ssm-simulator-fixtures). |
| [`public/synthetic/`](public/synthetic/) | Generated synthetic ROM images (`<plan>.bin`), their manifests (`<plan>.manifest.toml`) and `SHA256SUMS`. Never edit by hand: see [Synthetic ROMs](#synthetic-roms). |
| [`corpus-manifest.toml`](corpus-manifest.toml) | Manifest of the external stock-ROM corpus (D-025): path, size, SHA-256 and file-name fields of every image, metadata only. Images that are not a raw ROM of a standard size carry a `suspect` table (kind, reason, embedded-image offset); tests consuming the corpus must branch on it. Its `[defs]` table pins the ECU-definition corpus the same way (D-028: upstream, commit, licence position, every file's path, size and SHA-256, and the `logger_v370.zip` pin), metadata only. It lists ROM IDs by design, so it lives outside `public/` and `kat/`. Never edit by hand: `cargo xtask gen-corpus-manifest --mirror <dir> --defs-mirror <dir>` regenerates it and `--check` verifies it (format in [`xtask/src/corpus_manifest.rs`](../xtask/src/corpus_manifest.rs)). |
| [`defs-known-bad.toml`](defs-known-bad.toml) | The known-bad registry of the pinned definition corpus (US-RR-29-1, D-028): each pinned ECU definition whose Error diagnostics are the file's own defect, with its manifest SHA-256 (a corpus bump forces a re-review), every Error exactly (code, `line:col`, layer), the reason, and what legacy RomRaider did with it (checked in `legacy/`); and the root elements of pinned XML the ECU-definition reader does not read (`logger`, `dyno`), each with the reader that will. Metadata only. Edited by hand and reviewed; `crates/rr-defs/tests/corpus_romraider_loads.rs` validates it on every run (unknown keys, unpinned files, SHA mismatches, non-Error codes) and its corpus-tier test fails on any Error not listed, and on any listed Error no longer reported. |
| [`corpus-formulas.toml`](corpus-formulas.toml) | Every distinct formula of the pinned definition corpus (US-RR-20-10, D-028): the ECU `<scaling expression\|to_byte>` and logger `<conversion expr>` attributes, deduplicated by exact text, each as its SHA-256, occurrence count, kinds and first location (corpus-relative path and positional XPath), plus counts. Metadata only, never formula text. Never edit by hand: `RR_CORPUS_DIR=<dir> cargo run -p rr-testkit --example gen-corpus-formulas` regenerates it, and the corpus-tier test `corpus_formula_metadata_is_current` (CI `corpus` job) checks it (format in [`crates/rr-testkit/src/corpus/formulas.rs`](../crates/rr-testkit/src/corpus/formulas.rs)). |
| [`corpus-unit-strings.txt`](corpus-unit-strings.txt) | Every distinct `units` attribute value of the pinned definition corpus (US-RR-21-5, D-028): the ECU `<scaling units>` labels and logger `<conversion units>` strings, one per line as `<sources> \|<string>\|` (`defs` and/or `v370`). Metadata only: short labels, never formulas. rr-core's units tests read every string with `units::legacy_unit` and pin which ones warn (`UNT0001`). Never edit by hand: `RR_CORPUS_DIR=<dir> RR_UPDATE_UNIT_STRINGS=1 cargo nextest run -p rr-defs --test corpus_unit_strings` regenerates it, and that corpus-tier test (CI `corpus` job) fails when it is stale. |
| [`corpus-coverage.toml`](corpus-coverage.toml) | Which corpus images have a pinned ECU definition (US-RR-16-6, D-028): for every manifest image, the definition files whose `<romid>` matches its bytes (legacy RomRaider matching) with their `xmlid`s, or `no-definition`; the ROM ID read from the image; totals by region; the 2004 ADM Liberty flags. Metadata only, never definition content. Never edit by hand: `cargo xtask gen-corpus-coverage` regenerates it from a fetched corpus and `--check` verifies it (CI `corpus` job; format in [`xtask/src/corpus_coverage.rs`](../xtask/src/corpus_coverage.rs)). |
| [`corpus-rom-resolution.toml`](corpus-rom-resolution.toml) | Which definition each non-suspect corpus image resolves to through rr-defs' ROM-ID index (US-RR-34-1, EC-03, D-028): each pinned ECU-definition file taken as a set on its own (legacy loaded one file at a time; the pinned tree's standard, metric and MerpMod copies of one ROM loaded together are ambiguous by design), and for every image every file that matches it exactly once, with the `xmlid` it resolves to without Error (`base_from` when its base had to come from `ecu/standard/ecu_defs.xml`, MF-17) and the size check of opening the image through it (`ok`, `undeclared`, or `mismatch` with both sizes); `unique` or `no-definition`, and totals. It agrees with `corpus-coverage.toml` image by image, but for the differences the test lists with reasons. Metadata only. Never edit by hand: `RR_CORPUS_DIR=<dir> RR_UPDATE_ROM_RESOLUTION=1 cargo nextest run -p rr-rom --test corpus_rom_resolution` regenerates it; the corpus-tier test of [`crates/rr-rom/tests/corpus_rom_resolution.rs`](../crates/rr-rom/tests/corpus_rom_resolution.rs) (CI `corpus` job) fails when it is stale or any image is ambiguous or fails to resolve, and its no-corpus test checks its shape and its agreement with the coverage report on every run. |
| [`.github/workflows/fixtures.yml`](../.github/workflows/fixtures.yml) | The CI jobs that run the validator and check the synthetic ROMs on Linux and Windows. |

The seed set must cover every storage type in every byte order the codec
accepts (all but little-endian `u20`), a bitfield in each byte order, and
every non-default layout not listed as uncovered (today only
`bosch-subtract`, see [Open questions](#open-questions)), with at least 10
files. `crates/rr-testkit/tests/seed_coverage.rs` derives that set from the
code (`rr_core::encoding` and `rr_testkit::kat`) and fails when a supported
combination has no seed, so a new storage type or layout needs a seed fixture
in the same change.

The validator checks every `*.toml` file below the paths it is given. A
`.toml` file under `fixtures/kat/` is taken to be a KAT fixture, so keep
other TOML out of that directory.

## A minimal fixture

This fixture is complete and passes `cargo xtask validate-fixtures`:

```toml
schema = 1
title = "i16 big-endian, linear x*0.25: a positive and a negative cell"
notes = "Two's complement: value = b0*256 + b1, minus 65536 when b0 >= 0x80."

[definition.encoding]
storage = "i16"
endian = "big"

[definition.conversion]
to_real = "x*0.25"
to_raw = "x/0.25"
units = "deg"

[input]
hex = "01 2C FF 9C"

[[decode]]
offset = 0
raw = 300
real = 75.0
source = { kind = "hand-calc", working = "bytes 01 2C: 0x01*256 + 0x2C = 256 + 44 = 300; real = 300*0.25 = 75.0" }

[[decode]]
offset = 2
raw = -100
real = -25.0
source = { kind = "hand-calc", working = "bytes FF 9C: 255*256 + 156 = 65436; top bit set, so 65436 - 65536 = -100; real = -100*0.25 = -25.0" }
note = "reads 65436 if the cell is decoded as unsigned"

[[changelog]]
date = 2026-10-19
reason = "initial values"
```

Good complete examples in the seed set:

- [`kat/u16-be.toml`](kat/u16-be.toml): plain integer cells, including a
  top-bit-set case and a case that catches the wrong byte order.
- [`kat/scaling-linear-u16-be.toml`](kat/scaling-linear-u16-be.toml) and
  [`kat/scaling-nonlinear-u8.toml`](kat/scaling-nonlinear-u8.toml): `real`
  values, one exact and one with `tolerance`.
- [`kat/f32-be.toml`](kat/f32-be.toml): float working (sign, exponent and
  fraction), and a `standard` source for +Inf.
- [`kat/bitfield-u8.toml`](kat/bitfield-u8.toml): mask and shift.
- [`kat/u20-movi20.toml`](kat/u20-movi20.toml): the MOVI20 immediate, with
  bit 19 sign-extended (decided in US-RR-19, to match legacy).
- [`checksum_sum16.toml`](../crates/rr-testkit/tests/data/kat/checksum_sum16.toml):
  a checksum fixture.
- [`decode_rom_ref.toml`](../crates/rr-testkit/tests/data/kat/decode_rom_ref.toml):
  a ROM input, a definition reference, and the `cross-source` and `stock-rom`
  sources.

## Writing a fixture

The reference for every key is the module docs of
[`kat.rs`](../crates/rr-testkit/src/kat.rs). Unknown keys are errors, so a
typo cannot quietly drop a value. Here is the format in brief.

### Top level

| Key | Required | Meaning |
|-----|----------|---------|
| `schema` | yes | Always `1`. A missing, non-integer or unknown schema is rejected. |
| `title` | yes | One line saying what the fixture pins down. |
| `notes` | no | Free text for reviewers: the formula, the edge cases, why each value is interesting. |
| `[definition]` | for `[[decode]]` | How to read the bytes. |
| `[input]` | for `[[decode]]` and `[[checksum]]` | The bytes. An `[[expr]]`, `[[unit]]`, `[[timing]]` or `[[codec]]` fixture has none. |
| `[[decode]]`, `[[checksum]]`, `[[expr]]`, `[[unit]]`, `[[timing]]` or `[[codec]]` | yes, exactly one of the six | The expected values. A fixture is one shape, never two. |
| `[range]` | no, `[[decode]]` fixtures only | The conversion's display range. See [`[range]`](#range). |
| `[[changelog]]` | no | One entry each time an expected value changes. See [Changing an expected value](#changing-an-expected-value). |
| `[[citation]]` | for a standard test-vector fixture | The documents the values rest on, as structured fields. See [`[[citation]]`](#citation-standard-test-vector-fixtures). |

### `[definition]`

Use exactly one of these:

- **Inline encoding** (what every seed fixture uses):
  - `[definition.encoding]` takes `storage` (`u8 u16 u20 u24 u32 u64 i8 i16 i24 i32 i64 f32 f64`), `endian` (`big` | `little`, required even for one byte), `layout` (`default` if omitted, or `bosch-subtract`) and an optional `bitfield = { mask = 0x0FF0, shift = 4 }`. The shift defaults to the mask's trailing zeros, and the mask must be non-zero.
  - An optional `[definition.conversion]` takes `to_real` (an expression in `x`), and optionally `to_raw` and `units`. A case may only have `real` if there is a conversion.
- **Snippet:** `snippet = { format, text }`. `format` is one of `romraider-ecu-xml`, `romraider-logger-xml`, `xdf`, `ecuflash-xml` or `toml-profile`, and `text` is the definition text, verbatim. Do not add `conversion` (it is only allowed with an inline encoding).
- **Reference:** `reference = { tier, path, item }`. This points at a definition file in the `public`, `corpus` or `private` tier. `path` is relative to the tier root, and `item` (a table, scaling or parameter name) is optional.

A `[[checksum]]` fixture may leave out `[definition]`: the algorithm name and
its parameters are the definition.

### `[input]`

Use exactly one of these:

- `hex = "01 F4 FF 38"`: hex digits with optional whitespace, which can span
  lines in a `"""` string. Do not write a `0x` prefix. An odd number of digits
  is an error. Group the bytes by cell so a reviewer can find each one.
- `rom = { tier, path, sha256, offset, length }`: a slice of a ROM in the
  `corpus` or `private` tier. `sha256` is the digest of the whole ROM file (64
  hex digits), so the fixture cannot silently run against a different image.
  `offset` defaults to 0, and `length` defaults to the end of the file. The
  ROM bytes are never committed.

### `[[decode]]` cases

| Key | Meaning |
|-----|---------|
| `offset` | Byte offset of the cell in the input. Defaults to 0. With `hex` input and the default layout, the cell must fit inside the input. |
| `raw` | The expected raw value. Write an integer as a TOML integer. Write a float as a TOML float (`inf`, `-0.0` and `1.401298464324817e-45` all work). Write a `u64` above `i64::MAX` as a string such as `"0xFFFF_FFFF_FFFF_FFFF"`; strings accept `-`, `0x`, `0b` and `_`. |
| `real` | The expected value after conversion. Optional, and only allowed with a conversion. |
| `tolerance` | An absolute tolerance on `real`: finite, non-negative, and only allowed with `real`. Leave it out when the value is exact, for example for a scale that is a power of two. |
| `source` | Required. See [Sources](#sources). |
| `note` | Optional. Say what a wrong decoder would read, for example "reads 65280 if the byte order is wrong". |

### `[range]`

A decode fixture with a conversion may also pin the conversion's display
range: the smallest and largest real value over every raw value the cell can
hold (its storage's range, or its bitfield's), as an editor shows it.
`crates/rr-testkit/tests/conversion_kat.rs` checks every `[range]` against
rr-core's `Conversion::real_range`. It exists for falling scalings (US-RR-24-2,
PROJECT.md MF-13): legacy took a table's minimum as `f(lowest raw)` and its
maximum as `f(highest raw)` (`legacy/src/main/java/com/romraider/maps/Table.java:444-450`),
which swaps them for a decreasing scaling.

| Key | Meaning |
|-----|---------|
| `real_min` | The smallest real value. Finite. |
| `raw_at_min` | The lowest raw value giving `real_min`, written like a case's `raw`. |
| `real_max` | The largest real value. Finite. |
| `raw_at_max` | The lowest raw value giving `real_max`. |
| `monotonicity` | How the real value moves as the raw value rises: `increasing` (never falls, rises somewhere), `decreasing` (never rises, falls somewhere), `constant` or `non-monotonic`. |
| `tolerance` | An absolute tolerance on `real_min` and `real_max`, as for a case's `real`. |
| `source` | Required. A `hand-calc` working must reach all four values, so write it as "max = ... at raw R; min = ... at raw R", with why each extreme is where it is. |
| `note` | Optional. Say what a wrong range would be, for example "legacy's min/max were swapped here". |

Raw values with no real value (a division by zero) are left out of the
range; a `[range]` fixture should use a conversion defined over the whole raw
range. The validator checks that the range agrees with itself (rule `range`,
[below](#other-rules-the-validator-enforces)). The seed falling scalings are
[`scaling-decreasing-u8.toml`](kat/scaling-decreasing-u8.toml),
[`scaling-decreasing-u16-be.toml`](kat/scaling-decreasing-u16-be.toml),
[`scaling-decreasing-i16-le.toml`](kat/scaling-decreasing-i16-le.toml),
[`scaling-decreasing-u32-be.toml`](kat/scaling-decreasing-u32-be.toml) and
the reciprocal [`scaling-nonlinear-u8.toml`](kat/scaling-nonlinear-u8.toml);
[`scaling-linear-u16-be.toml`](kat/scaling-linear-u16-be.toml) is the rising
control. The schema has no lookup-table conversion, so no fixture pins a
table's range; rr-core's unit tests cover those.

### `[[checksum]]` cases

| Key | Meaning |
|-----|---------|
| `algorithm` | The checksum algorithm's name, for example `"sum16-be"`. |
| `params` | Optional parameters for the algorithm, for example `{ start = 0, end = 4 }`. |
| `offset` | Where the stored checksum lives in the input. It must fit inside a `hex` input. |
| `before` | `{ hex, valid, source }`: the stored checksum bytes and whether they verify, for the input as given. |
| `after` | The same, after the checksums are fixed. `before.hex` and `after.hex` must be the same length. |
| `note` | Optional. |

`before` and `after` each need their own `source`.

The validator does not check the `algorithm` name or its `params`: no
checksum algorithm registry exists yet (US-RR-35). The seed checksum
fixtures are the reference vectors below.

### Checksum reference vectors

The checksum and CRC reference vectors (US-RR-14-7, PROJECT.md T2 (d)) are
`[[checksum]]` fixtures, one family per file:
[`checksum-sum-bytes.toml`](kat/checksum-sum-bytes.toml) (byte sums mod 2^8
and 2^16, with an initial value),
[`checksum-sum-words.toml`](kat/checksum-sum-words.toml) (16- and 32-bit word
sums, big- and little-endian, one's and two's complement, Subaru's
0x5AA5A55A total), [`checksum-xor.toml`](kat/checksum-xor.toml) (byte and
32-bit word XOR) and [`checksum-crc.toml`](kat/checksum-crc.toml) (CRC-16
ARC, IBM-3740, XMODEM, KERMIT, MODBUS, IBM-SDLC, and CRC-32/ISO-HDLC). Each
file's header comment names the legacy algorithms (`file:line`) whose
arithmetic it pins. Conventions:

- They share one input: eight `0xFF` bytes, ASCII `"123456789"` (the CRC
  check input), and a four-byte zero slot at offset 17 where every case stores
  its result. `before` is that zero slot as given; `after` is the fixed value.
- `algorithm` is `sum`, `xor` or `crc`, and `params` spell out the arithmetic:
  the half-open region `start`/`end`, the `store` byte order, and for a sum
  `word`, `endian`, `width`, `init`, `negate` (`ones`, `twos`) or `target`
  (store `target - sum`); for a CRC the Rocksoft model parameters (`width`,
  `poly`, `init`, `refin`, `refout`, `xorout`) and the `catalogue` name. A
  region that is not a whole number of words needs `pad = "zero"` (legacy has
  no padding convention; see `checksum-sum-words.toml`).
- Sum and XOR values have no published standard: each is a `hand-calc`. CRC
  check values are `standard` sources citing the CRC RevEng catalogue entry
  (page, last-updated date, retrieval date, and the entry's parameter line);
  the CRCs of empty and all-0xFF input are worked bit by bit.

`crates/rr-testkit/tests/checksum_kat.rs` runs every vector through a small
reference implementation of its own (no rr-core, rr-rom or legacy code),
checks that every variant of its `PLANNED` list has a vector and that each
family has empty, all-0xFF, wrapping and odd-length vectors, and that every
CRC check value cites the catalogue entry matching its parameters. The
reference implementation is a cross-check, not a source: the values were
worked by hand or transcribed first.

### `[[expr]]` cases

An expression fixture pins down what a formula evaluates to. It has no
`[input]` and no `[definition]` (either is an error): the formula and `x`
are the whole question.

| Key | Meaning |
|-----|---------|
| `formula` | The formula, in the expression dialect of `rr_core::expr`. Only for a formula you may publish: one written for the fixture, or one from a published standard. |
| `formula_ref` | Instead of `formula`, for a formula of the definition corpus: `{ tier = "corpus", sha256, path, xpath }`. The definitions have no licence, so their formula text is never committed (D-028): `sha256` is the SHA-256 of the formula's UTF-8 text (`printf '%s' "$formula" \| sha256sum`) and identifies it; `path` (relative to the tier root, `defs/romraider/...`) and `xpath` (the attribute's positional XPath, `/roms[1]/rom[1]/table[3]/scaling[1]/@expression`) say where it occurs. `tier` is `corpus` or `private`, never `public`. Exactly one of `formula` and `formula_ref`. |
| `x` | The value of `x`. Defaults to 0. |
| `params` | The value of each parameter reference of a derived logger formula, by id: `params = { P9 = 90.0, P12 = 12.0 }` binds `[P9:km/h]` and `P12`. The keys must be exactly the formula's references. Defaults to none. |
| `value` | The expected value. `inf`, `-inf` and `nan` are allowed, for a documented non-finite result such as `1/0` or `(-8)^(1/3)`; rr-core reports those as `EvalError::NonFinite`. |
| `tolerance` | An absolute tolerance on `value`: finite, non-negative, and only for a finite `value`. Leave it out when the value is exact; an exact case is compared bit for bit, so `-0.0` is not `+0.0`. |
| `source` | Required. See [Sources](#sources). |
| `note` | Optional. Say what a wrong parse would give, for example "(2 + 3) * 4 would be 20". |

`crates/rr-testkit/tests/expr_kat.rs` parses and evaluates every `[[expr]]`
case of every fixture with rr-core. A `formula_ref` case needs the corpus: its
corpus-tier test `corpus_formula_cases_match_rr_core` harvests the fetched
definitions (`rr_testkit::corpus::formulas`), finds the formula by its hash,
checks that it occurs at `path` / `xpath`, and evaluates it; without
`RR_CORPUS_DIR` it is skipped and only the fixture's shape is checked. Keep
`formula_ref` cases in [`expr-corpus.toml`](kat/expr-corpus.toml).

To write one, find the formula in the fetched corpus (any XML tool, or the
`[[formulas]]` entries of [`corpus-formulas.toml`](corpus-formulas.toml) for
its hash and first location), work the value out by hand, and write the
working with the case's values substituted, as for any other hand
calculation. Never paste the formula text into the fixture, a comment or a
note.

### `[[unit]]` cases

A unit fixture pins down a conversion factor of rr-core's unit registry
(`rr_core::units`, concept C5, US-RR-21): a value in one unit and what it is
in another. Like an expression fixture it has no `[input]` and no
`[definition]`.

| Key | Meaning |
|-----|---------|
| `from` | The unit `value` is in: a unit id of the registry (`mph`, `fahrenheit`, `psi_gauge`), not a legacy unit string. |
| `to` | The unit `expected` is in, of the same dimension. |
| `value` | The value converted. Finite. |
| `expected` | `value` in `to`. Finite. |
| `tolerance` | An absolute tolerance on `expected`: finite and non-negative. Leave it out when the converted value is exact; an exact case is compared bit for bit. A factor with no finite decimal form (psi, hp) needs a few ulps. |
| `fuel` | Only for a case between an air/fuel ratio and lambda (`from = "afr"`, `to = "lambda"`, or the reverse), which needs it: the fuel whose stoichiometric ratio converts it, an id of `rr_core::units::FUELS` (`gasoline`, `methanol`). Say where the ratio comes from in the working or the file's comment. |
| `source` | Required. See [Sources](#sources). A factor a standard lists as exact is `standard` (NIST SP 811 Appendix B.8 marks exact factors in bold); one derived from exact definitions (1 psi = 1 lbf / 1 in²) is `hand-calc`, with the definitions in the working. |
| `note` | Optional. Say what a wrong conversion would give, for example "a pure factor with no offset would give 0". |

`crates/rr-testkit/tests/units_kat.rs` converts every `[[unit]]` case of every
fixture with `rr_core::units::convert` (or, for a case with a `fuel`,
`afr_to_lambda` / `lambda_to_afr`); a case whose ids name no registry unit or
fuel fails there, and so does a fuel with no case.

**Every conversion factor needs a cited known answer** (US-RR-21-1): the same
test fails, naming the unit or fuel, unless

- every registry unit that is not its dimension's base unit has a case
  converting between it and that base unit, in either direction (not through
  a third unit), at a value that is not 0 on the unit's side; an affine unit
  (kelvin, degree Fahrenheit) needs two such cases at different points;
- that case's `source` names its reference: NIST SP 811, the SI Brochure,
  NIST Handbook 44 or ISO 80000, in a `standard`'s `doc` or `section` or in a
  `hand-calc` working (`"SI Brochure 9th ed. (2019) Table 7: milli = 10^-3,
  so 250 mm = 250 / 1000 m = 0.25 m"`);
- every fuel has a case whose source cites Heywood for a pure compound, or,
  for a ratio that is legacy's convention (gasoline, diesel, LPG, CNG), says
  `no SI/NIST-equivalent reference` instead of inventing one;
- the case is exact, or its tolerance is within a relative 1e-12.

So a new unit or fuel cannot be added without its known answer.

### `[[timing]]` cases

A timing fixture pins down a protocol timing parameter a published standard
specifies (US-RR-14-5, PROJECT.md T2 (d)): each case names a constant of
`rr_core::timing::iso` and gives its value. Like an expression fixture it has
no `[input]` and no `[definition]`. One standard per file,
`timing-<standard>.toml`.

| Key | Meaning |
|-----|---------|
| `name` | The constant's path under `rr_core::timing::iso`, the standard's module and the parameter: `iso14230_2::P3_MIN`, `iso15765_2::N_BS_TIMEOUT`. |
| `ms` or `us` | The value, a whole number of milliseconds or microseconds: exactly one of the two. |
| `source` | Required, and a `standard`: `doc` names the document and its edition as `<document>:<year>` (`"ISO 14230-2:1999 (first edition, 1999-03-15)"`), `section` the clause and table or figure the value is printed in (`"4.5.1, Table 3: P3, minimum, default"`). |
| `note` | Optional. |

`crates/rr-testkit/tests/timing_kat.rs` checks every case against the
constant it names (so changing either alone fails), that every constant has
exactly one case, and that every source is a `standard` citing an edition
year and a section. Transcribe only values read in the cited edition and
clause; a parameter you cannot pin to one is left out and said so in the
file's comment, never written from memory or from legacy code.

### `[[codec]]` cases

A codec fixture pins down a byte-to-byte transform: each case is the bytes
in, an operation with its parameters, and the bytes out. Like an expression
fixture it has no `[input]` and no `[definition]`: each case carries its own
bytes. Today there is one, [`kat/ncs-codec-legacy.toml`](kat/ncs-codec-legacy.toml)
(US-RR-14-8): the NCS (Nissan) data codec and its CRC-16, transcribed from
the legacy test file, for the NCS codec of US-RR-78.

| Key | Meaning |
|-----|---------|
| `op` | The operation, no spaces: `ncs-encode`, `ncs-decode`, `ncs-crc16`. The names are the consuming test's, checked there, not by the validator. |
| `params` | Optional, operation-specific: `scode` (the 32-bit code) for `ncs-encode` / `ncs-decode`; `complement` and `store` (`big` / `little`, the byte order of the 16-bit result in `output`) for `ncs-crc16`. |
| `input`, `output` | Hex bytes, as in `[input] hex`. |
| `source` | Required. For a transcribed legacy test vector, `legacy-test` ([Sources](#sources)). |
| `note` | Optional. |

`crates/rr-testkit/tests/ncs_codec_kat.rs` checks the NCS file: one case per
legacy `@Test` method (six, pinned, and counted in the legacy file), each
citing lines that exist, include its test method and hold the transcribed
literals; it re-derives the four CRC outputs with its own implementation of
the CRC RevEng catalogue's CRC-16/MCRF4XX (the complement is
CRC-16/IBM-SDLC); and, since the encode and decode outputs cannot be
re-derived without porting the legacy routine, it checks only what holds
without one (decode inverts encode on the vector; equal plaintext words give
equal ciphertext words). The legacy file is read as text; nothing runs it.

### J1979 PID vectors

The SAE J1979 service $01 vectors (US-RR-14-6, PROJECT.md T2 (d)) are
`[[decode]]` fixtures with a few extra conventions, one scaling per file,
`j1979-01-<signal>-<storage>.toml`:

- `[input] hex` is positive service $01 responses, one per case: `0x41`
  (`0x01 + 0x40`), the PID echoed, then the data bytes A, B, ... Service-layer
  bytes only, no transport framing. Each case's `offset` is its byte A.
- The encoding is big-endian over all the PID's data bytes; a PID that
  carries two signals (oxygen sensor voltage and trim, wideband lambda and
  voltage or current, secondary trims of two banks) has one fixture per
  signal, picking its bytes with a `bitfield`, on the same responses.
- The conversion is the J1979 formula in `x`, with `units`.
- Each PID has a case with every data byte `0x00`, one with every data byte
  `0xFF`, and at least one typical value (a signed PID also its largest and
  smallest value).
- `notes` cites the formula: a `Standard: SAE J1979 ...` line, then a
  `Via (secondary): ...` line per source with its revision or retrieval date
  and the rows used, and a `Framing:` line. SAE J1979 and ISO 15031-5 are
  paywalled; the seed vectors take each formula from two secondary sources
  that agree (Wikipedia's "OBD-II PIDs" at a pinned revision, CSS
  Electronics' service 01 PID table) and say so. A PID whose sources
  disagree is left out (PID 54, signedness), as are the bit-encoded and
  enumerated PIDs, which have no formula.

The PIDs are those with a formula that legacy RomRaider's OBD logger offers
(the pinned logger definition's `<protocol id="OBD">`; each file's header
comment gives the line of each PID). `crates/rr-testkit/tests/j1979_kat.rs`
checks the framing, decodes every vector through rr-core's encoding and
conversion, and checks that every file cites its sources. Its US-RR-14-4
test pins the PID list against the headers' (82 PIDs), requires each
signal of each PID (two for 14–1B, 24–2B, 34–3B and 55–58, with disjoint
bitfields covering every data bit) to have its all-`0x00`, all-`0xFF` and
typical vectors, and requires the cases at the signal's lowest and highest
raw value to expect the `<lo> to <hi>` range on the `Formula:` line of
`notes`, so keep that line's form. The generic seed tests decode and convert
them too.

### `[[citation]]` (standard test-vector fixtures)

A standard test-vector fixture (US-RR-14) cites the documents its values
rest on in one or more top-level `[[citation]]` tables, as structured fields
(US-RR-14-1, validator rule `citation`). A case's `source` says where its
value came from (often a hand calculation); a citation says which document
defines what was calculated. A vector fixture is one whose file name starts
with `timing-`, `j1979-`, `checksum-` or `ncs-codec-`
(`validate::VECTOR_PREFIXES`), or any `[[timing]]` or `[[codec]]` fixture
whatever its name. Other fixtures may carry citations too; the rule then
checks them the same way but does not require one.

```toml
[[citation]]
kind = "standard"
doc = "ISO 14230-2"                       # the document, without its edition
edition = "1999 (first edition, 1999-03-15)"  # a four-digit year, or `version 3` / `rev. B`
section = "4.5.1 Value entering, Table 3" # the clause, table, figure or entry
```

| `kind` | Fields | Use it for |
|--------|--------|------------|
| `standard` | `doc`, `edition`, `section` (all non-empty; `edition` names a year 1900–2099 or a `rev`/`revision`/`version`/`issue` identifier), `verified`? (default `true`), `via`?, `note`? | A published standard or reference-vector catalogue (ISO, SAE, the CRC RevEng catalogue). |
| `legacy-test` | `file` (under `legacy/src/test/`), `lines` | Only in a `[[codec]]` fixture: the legacy test file the vectors are transcribed from (the NCS codec vectors). |
| `legacy-algorithm` | `file` (under `legacy/src/main/`), `lines`, `note` | Flagged exception, below. |

Per case, in a vector fixture: a `standard` source's `doc` must start with
the `doc` of one of its `standard` citations (`"ISO 14230-2:1999 (first
edition, ...)"` starts with `"ISO 14230-2"`), and a `legacy-test` source's
`file` must be the `file` of a `legacy-test` citation. A `hand-calc` case is
covered by the fixture's citations. That a cited legacy file exists and has
the cited lines is checked by `crates/rr-testkit/tests/vector_citations.rs`
(the validator reads fixtures only).

Two flagged exceptions, each pinned to named files in `validate.rs` so that
nothing else can use them:

- **`verified = false`** (`UNVERIFIED_CITATION_PREFIXES`: `j1979-01-*`
  only). SAE J1979 is paywalled: the PID formulas come from secondary
  sources, and the edition and section were not read first-hand. Such a
  citation must list those sources in `via` (each with its revision or
  digest), and says in `note` what is and is not known. `edition` is the
  edition Wikipedia lists, not one the values were checked against;
  `section` is the service and PID, not a clause or table number.
- **`legacy-algorithm`** (`LEGACY_ALGORITHM_FIXTURES`:
  `checksum-sum-bytes.toml`, `checksum-sum-words.toml`, `checksum-xor.toml`).
  No published standard defines these sums and XORs, so there is no standard
  to cite; the values are worked by hand, and the citation is the legacy
  code whose algorithm they model, for its semantics only (D-024). The
  `note` says so.

### Conventions from the seed set

- Start the file with a short comment saying how the values were derived and
  how they were cross-checked. The seed set uses "Cross-checked independently
  with Python struct.unpack (never with rr-core or rr-testkit code)".
- Use `notes` to state the formula once, so each case's `working` only has to
  apply it.
- Pick values that break a wrong implementation: zero, max, min, the top bit
  set, byte orders that differ, subnormal floats, `-0.0`. Each case should
  catch a different mistake.
- One storage type, layout or scaling per file. Keep the `-be` and `-le`
  fixtures of a storage type on the same bytes where you can, so they check
  each other.
- A new fixture starts with one `[[changelog]]` entry,
  `reason = "initial values"`. The validator does not require it.

## Sources

Every expected value has `source = { kind = "...", ... }`: each `[[decode]]`,
`[[expr]]`, `[[unit]]`, `[[timing]]` and `[[codec]]` case, each checksum `before` and `after`, and the `[range]`. A missing source is rule
`missing-source`. Pick the kind that matches where the number really came
from. If two kinds apply, use `cross-source`, or pick the stronger one and
mention the other in `note`.

| `kind` | Fields | Use it when |
|--------|--------|-------------|
| `hand-calc` | `working` | You worked the value out by hand from the bytes and the formula. This is the default for encodings and scalings. The working must meet [the working rule](#the-hand-calc-working-rule). |
| `standard` | `doc`, `section` (both non-empty) | A published document fixes the value: an IEEE 754 encoding, an SAE J1979 PID formula, an ISO timing value, or a published CRC or checksum reference vector. Name the document and edition in `doc`, and the section, table or vector in `section` precisely enough that a reviewer can find it. [`kat/f32-be.toml`](kat/f32-be.toml) uses this for +Inf. |
| `stock-rom` | `rom` (non-empty), `note`? | The value follows from a stock ROM invariant. For example: the stock ROM verifies, so its stored checksum is correct; or fix-checksums on a stock ROM is a no-op. `rom` is the ROM ID or file digest. The ROM itself stays in the corpus. |
| `car-trace` | `trace` (non-empty), `note`? | The value was observed on a real car, in a recorded trace. `trace` names the trace file or ID. Scrub VINs and serials first ([SECURITY.md, PII Handling](../.project/SECURITY.md#pii-handling)). |
| `cross-source` | `sources` (at least two distinct, non-empty) | Two or more independent sources agree, for example the RomRaider ECU XML and a TunerPro XDF for the same table. Each entry names one source. Two names for the same source do not count. |
| `legacy-test` | `file`, `lines` | Only in a `[[codec]]` case: the value is literal data of a legacy *test* file, transcribed by hand (US-RR-14 "NCS codec vectors transcribed as data from the legacy test file"). `file` is the path from the repository root, under `legacy/src/test/`; `lines` the lines the data is on, ascending numbers and ranges (`"33,35,37,39-43"`). Not a value the Java application computed: the test's own literals, which a consuming test can find on the cited lines. Say in the file header that the values are transcribed, and flag any vector that looks like legacy bug behaviour (cross-reference the Must-Fix ledger). |

**Never a source:**

- the code under test, or anything derived from it (rr-core, or rr-testkit's
  own decoder, once there is one), even "run once and copied";
- the Java RomRaider application. The validator rejects any source text that
  contains the word `java`, in any case, as rule `forbidden-source` (a
  `legacy-test` source's `file` path, which names a `.java` test file by
  design, is not searched). The check
  only finds the word, so values copied from Java's output without saying so
  are for the reviewer to catch.

You may cite `legacy/` source as `file:line` in `notes`, to explain the
*semantics* you are pinning down. For example, the seed
[`u20-movi20.toml`](kat/u20-movi20.toml) cites `RomAttributeParser.java` for
the cell layout. The value itself must still come from one of the kinds
above. A Python or calculator cross-check is fine as a reviewer aid. Mention
it in the header comment, but it is not a `source`.

### The `hand-calc` working rule

The validator enforces the rule `hand-calc-working`. After trimming, `working`
must:

1. **contain `=`**, so the working reads as an equation from the formula to
   the result;
2. **contain at least two numbers**: the result and at least one input or
   intermediate value. Numbers are decimals such as `244`, `-1`, `6533.6` or
   `1e-3`, or `0x`/`0b` prefixed such as `0x01F4`. Digits glued to a letter,
   as in `u16` or `x2`, are not numbers;
3. **show a formula**: at least one binary arithmetic or bitwise operator
   (`+ - * / % & | ^ << >> **`) between two operands, at least one of them a
   number or a parenthesised expression. A sign (`= -1`, `= +0.0`), a hyphen
   between words (`big-endian`), an exponent (`1e-3`) and a comparison
   (`0 < 128`) are not operators. Even a zero needs its formula:
   `"byte 0x00 = 0*16 + 0 = 0"`, or for a float zero
   `"value = (-1)^0 * 0/8388608 * 2^(1 - 127) = 1 * 0 = +0.0"`;
4. **arrive at the value it justifies**. For a `[[decode]]` case, the working
   must contain the expected `raw` value and, when the case has one, the
   expected `real` value. `real` matches within the case's `tolerance`, or else
   within a relative 1e-9. For an `[[expr]]` case, the working must contain
   `value` the same way, or, for a value that is not finite, the word `NaN`
   (any case) or `inf` / `infinity` (written `-inf` for `-inf`). For a
   `[[unit]]` case, the working must contain `expected` the same way, and for
   a `[[timing]]` case its `ms` or `us` value. For a `[[codec]]` case and a
   checksum `before` or `after`, the working must
   contain the stored checksum (the codec `output`), either as an integer (the bytes read big- or
   little-endian) or as its bytes in hex (`00 0A`, `0x000A`). For a
   `[range]`, the working must contain all four values: `real_min` and
   `real_max` (within its `tolerance`, or else a relative 1e-9),
   `raw_at_min` and `raw_at_max`;
5. **show an intermediate value**: at least one number that is not the
   result (for a `[range]`, not one of its four values). Every spelling of the result counts as the result: for
   `raw = 500`, `0x01F4` is 500, so `"0x01F4 = 500"` restates the answer.

A `hand-calc` source with no `working` key at all is also `hand-calc-working`.

| `working` (for `raw = 500, real = 50.0`) | Result |
|------------------------------------------|--------|
| `"500"`, `"trust me"` | rejected: no `=` |
| `"x = 500"` | rejected: only one number |
| `"500 = 500"`, `"raw = 500, real = 50.0"`, `"0x01F4 = 500 = 50.0"` | rejected: no formula (no operator) |
| `"0x01F4 = 1*256 + 244 = 499"` | rejected: never arrives at `real` 50.0 |
| `"500 - 500 + 500 = 500; 50.0 = 50.0"` | rejected: no intermediate value (every number is the result) |
| `"0x01F4 = 1*256 + 244 = 500; 500 * 0.1 = 50.0"` | passes |

The rule is a floor, not proof. Any number and any operator in the text
count, wherever they appear. The input bytes count too: `0x01F4` is 500, so
for a case without `real`, the wrong `"0x01F4 = 1*256 + 244 = 499"` would
pass. Reviewers reject working that only restates the answer, or that does not
add up. Show the bytes, the formula with the bytes substituted, and each
intermediate step, as the seed fixtures do.

### Other rules the validator enforces

The full list, with rule codes, is in
[`validate.rs`](../crates/rr-testkit/src/kat/validate.rs):

- `load`: the file must load as a schema-1 fixture (shape, keys, hex, dates).
- `empty-source`: the required fields of `standard`, `stock-rom` and
  `car-trace` sources must not be empty.
- `cross-source`: a `cross-source` needs at least two distinct sources.
- `legacy-test`: a `legacy-test` source only in a `[[codec]]` case, with a
  `file` under `legacy/src/test/` (relative, `/`-separated, no `.` or `..`)
  and `lines` as ascending numbers and ranges.
- `real-without-conversion` (also for a `[range]`) and `tolerance`.
- `range`: a `[range]` must agree with itself. `real_min` and `real_max` are
  finite and `real_min <= real_max` (a swapped pair is the MF-13 mistake);
  `increasing` needs `real_min < real_max` with `raw_at_min` below
  `raw_at_max`, `decreasing` the same with `raw_at_min` above `raw_at_max`,
  and `constant` needs `real_min == real_max`.
- `offset-out-of-range`: a cell or stored checksum must fit inside a `hex`
  input. This is not checked for `u20`, for the `bosch-subtract` layout, or for
  ROM input.
- `checksum-width`: `before.hex` and `after.hex` must be the same length.
- `changelog-reason`: a changelog `reason` must not be empty.
- `citation`: a standard test-vector fixture needs a `[[citation]]`, each
  case's `standard` source and `legacy-test` source names a cited document,
  and every citation is well formed (see
  [`[[citation]]`](#citation-standard-test-vector-fixtures)).

The validator checks the shape of the fixture and where its values came from,
not whether the values are correct. The independent reference codec
(`rr_testkit::reference`, US-RR-10) decodes every `[[decode]]` case of every
seed fixture (`crates/rr-testkit/tests/reference.rs`), and rr-core's encoding
(US-RR-19) will be tested against them too. Agreement with the reference
codec is a cross-check, not a source: a fixture's values are only as good as
its review.

## Changing an expected value

A changed expected value means a test was wrong, or the code is about to
change behaviour. Either way, it needs a reason in the file. Add a new
`[[changelog]]` entry in the same change:

```toml
[[changelog]]
date = 2026-11-02          # a plain TOML date
reason = "0x80 is -128 for i8, not 128: the old value read the cell as unsigned"
author = "your name"       # optional
```

In base mode (`--base <git-rev>`), the validator compares each fixture with
its version at the **merge-base** of `<git-rev>` and `HEAD`. A fixture that
git sees as renamed or moved since then (`git mv`, or any rename git detects
in the committed or staged change) is compared with its version under the old
path, so renaming a file does not hide a changed value. These count as an
expected-value change:

- the `[input]` or the `[definition]` changed (the same value now claimed for
  different bytes, a different ROM, or a different encoding);
- a base `[[decode]]` case (`offset`, `raw`, `real`, `tolerance`),
  `[[checksum]]` case (`algorithm`, `params`, `offset`, and the `hex` and
  `valid` of `before` and `after`), `[[expr]]` case (`formula`, `x`,
  `value`, `tolerance`, compared bit for bit, so a `nan` case matches itself),
  `[[unit]]` case (`from`, `to`, `fuel`, `value`, `expected`, `tolerance`)
  `[[timing]]` case (`name` and the duration; `ms = 1` rewritten as
  `us = 1000` is not a change) or `[[codec]]` case (`op`, `params`, `input`,
  `output`)
  no longer has an identical case. That is, it was changed or removed. A
  moved `offset` is a change, so swapping two cases' offsets is caught;
- a base `[range]` (`real_min`, `raw_at_min`, `real_max`, `raw_at_max`,
  `monotonicity`, `tolerance`) was changed or removed. Adding one is not a
  change;
- the fixture switched between `[[decode]]`, `[[checksum]]`, `[[expr]]`,
  `[[unit]]`, `[[timing]]` and `[[codec]]`.

When one of these happens, the new version must **add** a `[[changelog]]`
entry. Without one, the validator reports `changed-without-changelog` and
lists what changed. These do not count as adding an entry:

- an entry that was already in the base version;
- an entry with a blank `reason` (also reported as `changelog-reason`);
- a copy of an old entry, or a new entry with an old entry's reason;
- an old entry reworded, re-dated or replaced: the changelog is append-only,
  so the new version needs more added entries than base entries it edited or
  removed.

Cases are compared as a multiset, so **adding or reordering cases is not a
change**. Editing a source, note, title or the file's comments needs no
changelog entry either.

Not caught by this check, so left to review: a new fixture (including a
rename so heavily edited that git no longer sees it as a rename; it is then
checked as a new fixture, so every value needs a source); a fixture whose
base version does not load; a deleted fixture (there is no file left to carry
the changelog line); and a change inside an external file a fixture refers to
(an `[input] rom` is compared by its reference and SHA-256, a definition
`ref` by its reference only).

The changelog check is not a substitute for review. A changed value also
needs a reviewer to recompute it ([Review](#review)).

### In CI

[`.github/workflows/fixtures.yml`](../.github/workflows/fixtures.yml) runs on
every push and pull request that touches `fixtures/`, `crates/rr-testkit/`,
`xtask/`, `Cargo.toml`, `Cargo.lock`, `.cargo/config.toml`,
`rust-toolchain.toml`, `.gitattributes` or the workflow itself. Its
`validate` job runs two steps on a full-history checkout (its `synthetic`
job is described in [Synthetic ROMs](#synthetic-roms)):

1. `cargo xtask validate-fixtures`, on every fixture;
2. base mode:
   - on a pull request, `cargo xtask validate-fixtures --base "$base"` with
     the PR's base commit;
   - on a push (or a manual run), `cargo xtask validate-fixtures
     --push-before="$BEFORE"` with the push's `github.event.before`. The base
     is that commit when the checkout has it. When it has not (a new
     branch's first push, where it is all zeros, or a force push whose old
     head is gone) the base is where `HEAD` leaves every other remote branch:
     the parent of the oldest pushed commit that is on no other branch. So
     the pushed commits are always checked. (The default branch, `master`,
     has no fixtures, so comparing with it would check nothing.)

   A base that cannot be resolved exits 2 and fails the job.

The workflow starts on every push and pull request. Its `scope` job skips
the fixture jobs when the change touches none of the fixture paths, and its
`fixtures-ok` job always reports. `fixtures-ok` is a required status check on
`rust-rewrite`, next to `ci-ok` (CONTRIBUTING.md "Branch protection").

## Running the validator locally

```sh
export PATH=$HOME/.cargo/bin:$PATH        # if cargo is not already on PATH

cargo xtask validate-fixtures                                  # every fixture: fixtures/kat and crates/rr-testkit/tests/data/kat
cargo xtask validate-fixtures fixtures/kat/u16-be.toml         # one file (or a directory: every *.toml below it)
cargo xtask validate-fixtures --base origin/rust-rewrite       # also enforce the changelog rule against the branch you will merge into
cargo xtask validate-fixtures --push-before=                   # what CI does on a new branch: compare with where HEAD leaves the other remote branches
```

Paths are relative to the directory you run the command from. With
`--base`, the git revision is resolved in the repository that contains that
directory.

The exit status is 0 when every fixture passes, 1 when there is any
violation, and 2 on a usage error, an unreadable file or an unknown base
revision. Violations go to stderr, one per line, with the file, the case
(cases are numbered from 1) and the rule code:

```text
validate-fixtures: fixtures/kat/x.toml: [[decode]] #1 (offset 0): expected value has no `source`; ... [missing-source]
validate-fixtures: 1 violation(s) in 1 fixture(s)
```

On success it prints, for example,
`validate-fixtures: ok (33 fixture(s), 2 compared with origin/master (1a2b3c4d5e6f))`.

`cargo xtask validate-fixtures` builds and runs rr-testkit's
`validate-fixtures` example. rr-testkit is a dev-dependency only, so xtask
cannot link it. The validator's own tests are in
`crates/rr-testkit/tests/kat_validate.rs`, and they run under
`cargo nextest run --workspace`.

## Definition-reader fixtures

[`defs-kat/romraider/`](defs-kat/romraider/) holds the known-answer
fixtures of the RomRaider ECU-definition reader (story US-RR-29, task
US-RR-29-2): one file per table kind, and one for `base` / `omit`
inheritance. Each is a TOML file with `schema = 1` holding:

- `xml`: a whole, minimal definition file, verbatim, in a literal string
  (`'''`). Its line 1 is the line after the opening `'''`, so positions
  are the XML's own `line:col`.
- `resolve`: the `xmlid` of the `<rom>` to resolve, along its `base` chain.
- `diagnostics` (every diagnostic, exactly: `code`, `severity`, `at`),
  `unplaced` (tables with no address in any layer) and `omitted` (tables an
  `omit="true"` removed). Each defaults to none.
- `[[table]]`: every resolved table, in order. Its keys are the merged
  `TableDoc` fields (a key left out is a field the XML does not set), `at`
  and `layer` (the `<table>` element and `<rom>` it comes from), its
  `[table.cells]` (with `[table.cells.scaling.<category>]`),
  `[[table.state]]`, `[[table.bit]]` and `[[table.axis]]` (as resolved:
  the one axis of a 2D table, X then Y of a 3D one), and the resolved
  `[table.encoding]` and `[table.conversion]`.
- `[[table.cell]]` (and `[[table.axis.cell]]`): `hex` bytes of one cell, and
  the `real` value the resolved encoding and conversion must decode them to.

Every derived value (an `encoding`, a `conversion`, a `cell`) has a
`working` that shows how it follows from the XML, with an `=`: defaults
(big-endian, a switch's one-byte storage), a `<memmodel endian>` override,
the bytes and arithmetic of a decoded cell. The same no-Java rule applies:
no expected value comes from the reader or from the Java application.

The test that reads them,
[`crates/rr-defs/tests/kat_romraider_tables.rs`](../crates/rr-defs/tests/kat_romraider_tables.rs),
documents every key. It loads each file with rr-testkit's schema-checking
loader, rejects unknown keys and a `working` without `=` or citing Java,
compares the reader's answer field by field (a field the reader sets that
the fixture does not list fails too), decodes every `cell`, and checks that
the fixtures cover every table kind and axis type. `cargo xtask
validate-fixtures` does not read this directory: its rules are those of the
byte-decoding fixtures in `kat/`. Review them as any fixture
([Review](#review)): recompute each `working` by hand from the XML.

## SSM simulator fixtures

[`public/ssm/`](public/ssm/) holds SSM (Subaru Select Monitor) exchanges on
a K-line, written from [`docs/legacy/ssm.md`](../docs/legacy/ssm.md) only
(task US-RR-12-4): init, `A8` address reads (once and continuous), `A0`
block reads, `B8` and `B0` writes with their acknowledgements, reset, and
error cases. They are the starting scripts of the ECU simulator (US-RR-36)
and test vectors for the SSM codec (US-RR-47). Every ECU ID, address and
value is made up; nothing comes from a real car or ROM, and the ROM-leak
guard scans this directory like the rest of `public/`.

Each file has `schema = 1`, `kind = "ssm-kline-sim"`, a `title`, the
`fixture_ids` of ssm.md's "Fixtures to write" list it serves, a `synthetic`
note saying what is made up, `[[changelog]]` entries, and one
`[[exchange]]` per request/reply cycle:

| Key | Meaning |
|-----|---------|
| `name`, `spec` | Case name; the ssm.md sections it follows (`docs/legacy/ssm.md §3.3, §1.4`) |
| `module` | `ecu` (address `10`) or `tcu` (`18`) |
| `command` | `BF`, `A8`, `A0`, `B8` or `B0` |
| `state` | `single-shot` (request written, echo, one reply), `continuous-first` (an `A8` with `PP = 01`: request, echo, first frame) or `continuous-stream` (no request, frames only) |
| `tx`, `tx_checksum` | The request as the tester writes it (empty for a stream), and its checksum working |
| `rx`, `rx_checksum` | The frames the module sends, and each one's checksum working (none for a cut-short frame) |
| `wire` | Every byte on the K-line in order, which is what the tester's receive buffer holds: the echo of `tx`, then `rx` |
| `outcome` | `ok`, or `error` with `error` (`bad-checksum`, `short-reply`, `silent`, `echo-mismatch`, `no-echo`), `legacy` (what the Java does, per ssm.md) and `rr` (what the port must do) |
| `meaning` | What an `ok` exchange means, in words |
| `[exchange.decoded]` | The decoded content, by command: `pp` and `addresses` / `values` (`A8`, one `values` entry per frame); `pp`, `address`, `count`, `data` (`A0`); `address`, `value` (`B8`); `address`, `data` (`B0`); `init_data`, `ecu_id`, `capability` (`BF`) |

Bytes are space-separated hex. A checksum working lists every byte before
the checksum, then the sum and its low 8 bits:
`80+10+F0+01+BF = 0x240; low 8 bits 40`.

[`crates/rr-testkit/tests/ssm_fixtures.rs`](../crates/rr-testkit/tests/ssm_fixtures.rs)
checks every exchange against ssm.md's rules, implemented there
independently of any codec: header, IDs, `LL` = frame length − 5, the
8-bit sum and its working, response code = command + `40`, each command's
parameter layout and limits (`A8` N ≤ 84, `A0` count 1–254, `B0` n ≤ 251),
the reply that belongs to each request, the ECU ID and capability bits,
the echo (`wire` = `tx` then `rx`), the `A8` buffer size
`(N+6) + (3N+7)`, and that each error case has exactly the defect it
names. It also checks that the cited ssm.md sections and fixture IDs
exist. `cargo xtask validate-fixtures` does not read this directory.

## Synthetic ROMs

[`public/synthetic/`](public/synthetic/) holds ROM images built from known
values by the synthetic ROM generator
([`crates/rr-testkit/src/reference/synthetic.rs`](../crates/rr-testkit/src/reference/synthetic.rs),
story US-RR-10), so public tests have ROM-shaped data without a stock ROM.
Each published plan (`synthetic::published_plans()`, today only `standard`)
produces two files:

- `<plan>.bin`: the ROM image. The values are written with the independent
  reference codec, never with rr-core. The standard plan covers the min and
  max of every storage type in both byte orders, NaN (quiet and signalling,
  distinct payloads), ±Inf, ±0, subnormals, bitfield and Bosch-subtract
  tables, overlapping tables and a table that ends exactly at the end of
  the ROM.
- `<plan>.manifest.toml`: every table (address, storage, endian, layout,
  bitfield, dims) and the raw value every cell must decode to, decoded from
  the final ROM. Its format is in the generator's module docs.

`SHA256SUMS` lists the SHA-256 of both, in `sha256sum` format.

**These files are generated. Do not edit them by hand.** Change the plan in
`synthetic.rs` instead, then regenerate and commit the result:

```sh
cargo xtask gen-synthetic           # rewrite fixtures/public/synthetic (removes files no plan generates)
cargo xtask gen-synthetic --check   # write nothing; fail on any byte difference
(cd fixtures/public/synthetic && sha256sum -c SHA256SUMS)   # the same check, without our code
```

`cargo xtask gen-synthetic` runs rr-testkit's `gen-synthetic` example into a
scratch directory (rr-testkit is a dev-dependency only, so xtask cannot link
it), adds `SHA256SUMS`, and copies the result into place. With `--check` it
compares the files byte for byte with what is committed and exits 1 on a
missing, extra or different file. The generator is deterministic: no hash
maps, timestamps, paths or float text, so the output is identical from run to
run and on every platform.

The `synthetic` job in
[`.github/workflows/fixtures.yml`](../.github/workflows/fixtures.yml) runs
`cargo xtask gen-synthetic --check` and `sha256sum -c SHA256SUMS` on both
`ubuntu-24.04` and `windows-2022`. The directory's
[`.gitattributes`](public/synthetic/.gitattributes) marks `*.bin` as
`binary` and the manifest and `SHA256SUMS` as `text eol=lf`, so a Windows
checkout with `core.autocrlf=true` cannot rewrite the bytes. If `--check`
reports `\r\n` line endings, your checkout predates that file or overrides
it.

**How a synthetic ROM is recognisable.** Every generated ROM starts with the
32-byte marker `RomRaider-RS SYNTHETIC ROM v1` (padded with NUL bytes) at
offset 0, which no table may overwrite, and its manifest says
`synthetic = true`. The ROMs are small (the standard ROM is 2 KiB, well under
the 64 KiB limit of the ROM-leak guard). The guard (`cargo xtask leak-guard`,
US-RR-13) relies on the marker: a binary that starts with it is exempt from
the ROM-ID-address rule, never from the known-ROM-ID rule. Any `.bin` in `public/synthetic/` that is not produced by a
published plan fails `--check`, so a real ROM cannot be hidden there.

## Review

Every new or changed fixture is reviewed in its PR. The reviewer:

1. **Recomputes each expected value independently**, from the bytes in the
   fixture and the formula in `notes` (for a `formula_ref`, the formula read
   from the fetched corpus at `path` / `xpath`), by hand or with a throwaway
   tool such as a Python one-liner. Never use rr-core, rr-testkit's own code, the Java
   application, or the author's script. If the recomputed value differs, the
   fixture is wrong until shown otherwise.
2. **Checks that the working is real working:** it shows the bytes, the
   formula with the values substituted, and each intermediate step. It must
   not just restate the answer.
3. **Checks the source kind is honest.** A `standard` names a document and
   section that really say this. A `cross-source` lists sources that really are
   independent. Nothing comes, directly or indirectly, from the code under test
   or from Java (CONTRIBUTING.md,
   [No-Java rule](../CONTRIBUTING.md#no-java-rule-d-024)).
4. **For a changed value, reads the `[[changelog]]` reason** and agrees that
   the old value was wrong, or that the behaviour change is intended. A
   deliberate departure from legacy behaviour is also recorded in the Must-Fix
   ledger
   ([PROJECT.md](../.project/PROJECT.md#must-fix-bugs-from-the-java-codebase)).
5. **Checks that the cases are useful.** They include the edges that break a
   wrong decoder (sign, byte order, max, zero), and no case depends on an
   undecided behaviour ([Open questions](#open-questions)).
6. **Checks that no ROM bytes are committed.** A `hex` input is synthetic, or a
   short scrubbed excerpt; anything bigger uses `rom = { ... }` with a corpus
   or private tier.
7. Confirms `cargo xtask validate-fixtures` passes, and the Fixtures workflow
   is green.

## Open questions

- **`bosch-subtract` layout and checksums.** The format can express both, but
  there are no seed fixtures for them yet, and no checksum algorithm registry
  to check `algorithm` names against. A `bosch-subtract` value depends on
  every later cell of its table, and the format has no way yet to say where
  the table ends, so the reference codec's KAT test rejects such a fixture
  until that is defined. The layout's semantics (from legacy `DataCell.java`)
  and hand-calculated checks live with the reference codec
  (`crates/rr-testkit/src/reference.rs`, `tests/reference.rs`).
