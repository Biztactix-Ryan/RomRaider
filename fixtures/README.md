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
([below](#input)), and the bytes stay out of the repository. A guard that
rejects ROM images under `fixtures/` is planned (story US-RR-13), but it is
not built yet. Until then, reviewers check this by hand. The only ROM-shaped
files in the repository are the generated synthetic ROMs in
[`public/synthetic/`](public/synthetic/), which are built from known values
and marked as synthetic ([Synthetic ROMs](#synthetic-roms)).

## Layout

| Path | What |
|------|------|
| [`fixtures/kat/*.toml`](kat/) | Seed KAT fixtures, one storage type or scaling per file, named `<storage>-<endian>.toml` (`u16-be.toml`, `i24-le.toml`, `u8-be.toml`), plus `bitfield-*`, `scaling-*` and `u20-movi20.toml`. |
| [`crates/rr-testkit/tests/data/kat/*.toml`](../crates/rr-testkit/tests/data/kat/) | Samples for rr-testkit's own schema tests: a checksum fixture, a decode fixture with an inline conversion, and one that uses a ROM and definition reference. They are not seed fixtures, but the validator checks them too. |
| [`crates/rr-testkit/src/kat.rs`](../crates/rr-testkit/src/kat.rs) | The schema-1 types (serde model). Its module docs are the format reference. |
| [`crates/rr-testkit/src/kat/validate.rs`](../crates/rr-testkit/src/kat/validate.rs) | The validator rules. Its module docs are the reference for every rule. |
| [`public/synthetic/`](public/synthetic/) | Generated synthetic ROM images (`<plan>.bin`), their manifests (`<plan>.manifest.toml`) and `SHA256SUMS`. Never edit by hand: see [Synthetic ROMs](#synthetic-roms). |
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
| `[input]` | yes | The bytes. |
| `[[decode]]` or `[[checksum]]` | yes, exactly one of the two | The expected values. A fixture is one shape or the other, never both. |
| `[[changelog]]` | no | One entry each time an expected value changes. See [Changing an expected value](#changing-an-expected-value). |

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

The validator does not check the `algorithm` name yet. No checksum algorithm
registry exists, and no seed checksum fixture exists either; the only
checksum fixture is the schema sample.

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

Every expected value has `source = { kind = "...", ... }`: each `[[decode]]`
case, and each checksum `before` and `after`. A missing source is rule
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

**Never a source:**

- the code under test, or anything derived from it (rr-core, or rr-testkit's
  own decoder, once there is one), even "run once and copied";
- the Java RomRaider application. The validator rejects any source text that
  contains the word `java`, in any case, as rule `forbidden-source`. The check
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
   within a relative 1e-9. For a checksum `before` or `after`, the working must
   contain the stored checksum, either as an integer (the bytes read big- or
   little-endian) or as its bytes in hex (`00 0A`, `0x000A`);
5. **show an intermediate value**: at least one number that is not the
   result. Every spelling of the result counts as the result: for
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
- `real-without-conversion` and `tolerance`.
- `offset-out-of-range`: a cell or stored checksum must fit inside a `hex`
  input. This is not checked for `u20`, for the `bosch-subtract` layout, or for
  ROM input.
- `checksum-width`: `before.hex` and `after.hex` must be the same length.
- `changelog-reason`: a changelog `reason` must not be empty.

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
- a base `[[decode]]` case (`offset`, `raw`, `real`, `tolerance`) or
  `[[checksum]]` case (`algorithm`, `params`, `offset`, and the `hex` and
  `valid` of `before` and `after`) no longer has an identical case. That is, it
  was changed or removed. A moved `offset` is a change, so swapping two
  cases' offsets is caught;
- the fixture switched between `[[decode]]` and `[[checksum]]`.

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
the 64 KiB limit of the planned ROM-leak guard). The guard (US-RR-13) can rely
on the marker. Any `.bin` in `public/synthetic/` that is not produced by a
published plan fails `--check`, so a real ROM cannot be hidden there.

## Review

Every new or changed fixture is reviewed in its PR. The reviewer:

1. **Recomputes each expected value independently**, from the bytes in the
   fixture and the formula in `notes`, by hand or with a throwaway tool such as
   a Python one-liner. Never use rr-core, rr-testkit's own code, the Java
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
