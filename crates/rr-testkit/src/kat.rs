//! Known-answer (KAT) fixture schema, version 1 (US-RR-9, PROJECT.md T2,
//! ARCHITECTURE.md "Contracts" → "Fixture format", D-024).
//!
//! A KAT fixture is a hand-authored, reviewed TOML file holding an input
//! (hex bytes, or a reference to a ROM file), a definition (an inline
//! encoding, a definition-file snippet, or a reference to a definition file),
//! and the expected answers: either decoded raw/real values or checksum state
//! before and after a fix. A third shape, the expression fixture, has no
//! bytes at all: each case is a formula, a value of `x` and the value the
//! formula evaluates to. **Every expected value carries a [`Source`]** that
//! says where the number came from; values derived from the code under test,
//! or from the Java application, are not a source (D-024).
//!
//! These types are the serde model only. They make the structural rules
//! unrepresentable to break — a missing `source`, a `hand-calc` source without
//! `working`, an unknown key, malformed hex, a fixture with both or neither
//! case kinds, an input with both or neither of `hex`/`rom` — and every such
//! fixture fails to load with an error naming the file and the problem.
//! Semantic checks (working that actually shows the formula, a cross-source
//! citing at least two sources, offsets within the input, `real` only with a
//! conversion, ...) belong to the validator built on top of these types,
//! [`validate`] (`cargo xtask validate-fixtures`).
//!
//! The vocabulary ([`Storage`], [`Endian`], [`Layout`], ...) is local to
//! rr-testkit on purpose: KAT fixtures, and the reference codec that reads
//! them, must stay independent of rr-core's encoding (PROJECT.md T2 (b)).
//!
//! # Decode-values fixture
//!
//! ```toml
//! schema = 1
//! title = "u16 big-endian, linear 0.1 degC"
//! notes = "optional free text"
//!
//! [definition.encoding]           # or `snippet = {..}` / `reference = {..}`
//! storage = "u16"                 # u8 u16 u20 u24 u32 u64 i8 i16 i24 i32 i64 f32 f64
//! endian = "big"                  # big | little
//! layout = "default"              # default (if omitted) | bosch-subtract
//! bitfield = { mask = 0x0FF0, shift = 4 }   # optional; shift defaults to mask's trailing zeros
//!
//! [definition.conversion]         # optional
//! to_real = "x * 0.1"
//! to_raw = "x / 0.1"              # optional
//! units = "degC"                  # optional
//!
//! [input]
//! hex = "01 F4 FF 38"             # or rom = { tier, path, sha256, offset, length }
//!
//! [[decode]]
//! offset = 0                      # byte offset into the input (default 0)
//! raw = 500                       # integer, float, or "0x.." / "-12" string
//! real = 50.0                     # optional
//! tolerance = 1e-9                # optional absolute tolerance on `real`
//! source = { kind = "hand-calc", working = "0x01F4 = 1*256 + 244 = 500; 500 * 0.1 = 50.0" }
//!
//! [range]                         # optional; needs a conversion (US-RR-24-2, MF-13)
//! real_min = 0.0                  # the smallest real value over every raw value the cell holds
//! raw_at_min = 0                  # the lowest raw value giving it
//! real_max = 6553.5               # the largest real value
//! raw_at_max = 65535              # the lowest raw value giving it
//! monotonicity = "increasing"     # increasing | decreasing | constant | non-monotonic
//! tolerance = 1e-9                # optional absolute tolerance on `real_min` / `real_max`
//! source = { kind = "hand-calc", working = "0*0.1 = 0.0 at raw 0; 65535*0.1 = 6553.5 at raw 65535" }
//!
//! [[changelog]]                   # optional; one entry per change to an expected value
//! date = 2026-10-19
//! reason = "why the old value was wrong"
//! author = "optional"
//! ```
//!
//! # Checksum fixture
//!
//! ```toml
//! schema = 1
//! title = "16-bit additive checksum over 4 bytes"
//!
//! [input]
//! hex = "01 02 03 04 00 00"
//!
//! [[checksum]]
//! algorithm = "sum16-be"          # named algorithm (compiled-in registry name)
//! params = { start = 0, end = 4 } # optional, algorithm-specific
//! offset = 4                      # where the stored checksum lives in the input
//! before = { hex = "00 00", valid = false, source = { kind = "hand-calc", working = "stored bytes are zero; sum is 10" } }
//! after = { hex = "00 0A", valid = true, source = { kind = "hand-calc", working = "1+2+3+4 = 10 = 0x000A" } }
//! ```
//!
//! A checksum fixture's `[definition]` is optional (the algorithm and params
//! are its definition); a decode fixture must have one.
//!
//! A decode fixture may also pin its conversion's *display range*
//! (`[range]`, [`RangeCase`]): the smallest and largest real value over
//! every raw value the cell can hold (its storage, or its bitfield), the
//! lowest raw value giving each, and whether the conversion rises or falls.
//! It exists for decreasing scalings, whose minimum is at the top of the raw
//! range (legacy took the minimum at the lowest raw value, MF-13). Only a
//! decode fixture has one.
//!
//! # Expression fixture
//!
//! ```toml
//! schema = 1
//! title = "JEP precedence: * binds tighter than +"
//!
//! [[expr]]
//! formula = "2 + 3*x"             # a formula in the expression dialect (rr-core::expr)
//! x = 4.0                         # optional; the value of `x` (default 0)
//! value = 14.0                    # expected value; `inf`, `-inf` and `nan` are allowed
//! tolerance = 1e-15               # optional absolute tolerance on a finite `value`
//! source = { kind = "hand-calc", working = "2 + (3*4) = 2 + 12 = 14" }
//! note = "optional"
//!
//! [[expr]]                        # a formula of the definition corpus, named by hash (D-028)
//! formula_ref = { tier = "corpus", sha256 = "<64 hex digits>", path = "defs/romraider/logger/logger_STD_EN_v290.xml", xpath = "/logger[1]/.../conversion[1]/@expr" }
//! params = { P9 = 90.0, P12 = 12.0 }  # optional; the value of each parameter reference, by id
//! value = 1.5
//! source = { kind = "hand-calc", working = "..." }
//! ```
//!
//! An expression fixture has no `[input]` and no `[definition]`: the formula,
//! `x` and `params` are the whole question. A case has exactly one of
//! `formula` (the text, inline) and `formula_ref` (a formula of the
//! definition corpus, whose text has no licence and so is never committed:
//! the SHA-256 of its UTF-8 text identifies it, `path` and `xpath` say where
//! it occurs; [`crate::corpus::formulas`]). Decode and checksum fixtures must
//! have an `[input]`. A fixture is exactly one of the six shapes.
//!
//! # Unit-conversion fixture
//!
//! ```toml
//! schema = 1
//! title = "Length and speed: the international mile"
//!
//! [[unit]]
//! from = "mph"                    # a unit id of rr-core's unit registry (C5)
//! to = "kmh"
//! value = 1.0                     # the value in `from`
//! expected = 1.609344             # the value in `to`; finite
//! tolerance = 1e-15               # optional absolute tolerance on `expected`
//! source = { kind = "standard", doc = "NIST SP 811 (2008)", section = "Appendix B.8, mile: 1.609 344 E+03 m (exact)" }
//! note = "optional"
//! ```
//!
//! A unit fixture (US-RR-21) pins down a conversion factor of the unit
//! registry: each case is a value in one unit and what it is in another.
//! Like an expression fixture it has no `[input]` and no `[definition]`.
//! The ids are the registry's (rr-core's `units::by_id`), checked by
//! rr-testkit's `units_kat` test, not here: the fixture types do not name
//! rr-core.
//!
//! A case converting between an air/fuel ratio and lambda (`from = "afr"`,
//! `to = "lambda"` or the reverse) also names the fuel whose
//! stoichiometric ratio it uses, `fuel = "gasoline"` (an id of rr-core's
//! `units::FUELS`, US-RR-21-5); no other case has one. The `units_kat`
//! test checks both.
//!
//! # Timing fixture
//!
//! ```toml
//! schema = 1
//! title = "ISO 14230-2:1999 default timing parameters P1 to P4"
//!
//! [[timing]]
//! name = "iso14230_2::P3_MIN"     # a constant under rr-core's timing::iso
//! ms = 55                         # the value: exactly one of `ms` and `us`
//! source = { kind = "standard", doc = "ISO 14230-2:1999 (first edition, 1999-03-15)", section = "4.5.1, Table 3, P3 minimum default" }
//! note = "optional"
//! ```
//!
//! A timing fixture (US-RR-14) pins down a protocol timing parameter a
//! standard specifies: each case names a constant and gives its value as a
//! whole number of milliseconds (`ms`) or microseconds (`us`). Like an
//! expression fixture it has no `[input]` and no `[definition]`. The names
//! are rr-core's (`timing::iso`), checked by rr-testkit's `timing_kat`
//! test, not here: the fixture types do not name rr-core.
//!
//! # Codec fixture
//!
//! ```toml
//! schema = 1
//! title = "NCS codec vectors transcribed from the legacy test"
//!
//! [[codec]]
//! op = "ncs-encode"               # the transform; names are the consuming test's, not checked here
//! params = { scode = 0xC2C0823F } # optional, op-specific
//! input = "FF FF 22 E4"           # hex bytes in
//! output = "11 82 83 C9"          # hex bytes out
//! source = { kind = "legacy-test", file = "legacy/src/test/java/com/romraider/maps/checksum/NcsCoDecTest.java", lines = "33,35,37,39-43" }
//! note = "optional"
//! ```
//!
//! A codec fixture (US-RR-14-8) pins down a byte-to-byte transform: each
//! case is the bytes in, the operation and its parameters, and the bytes
//! out. Like an expression fixture it has no `[input]` and no
//! `[definition]`: each case carries its own bytes. The `op` names are the
//! consuming test's (rr-testkit's `ncs_codec_kat` today), checked there, not
//! here.
//!
//! # Sources
//!
//! | `kind` | Fields | Meaning |
//! |--------|--------|---------|
//! | `hand-calc` | `working` | Calculated by hand; `working` shows the formula and intermediate values |
//! | `standard` | `doc`, `section` | A published standard or reference vector |
//! | `stock-rom` | `rom`, `note`? | A stock-ROM invariant (e.g. the stock ROM verifies) |
//! | `car-trace` | `trace`, `note`? | Observed in a recorded car trace |
//! | `cross-source` | `sources` | Agreement between independent sources (e.g. RomRaider XML vs XDF) |
//! | `legacy-test` | `file`, `lines` | Literal data transcribed from a legacy *test* file (`legacy/src/test/...`, the lines it is on); only in a `[[codec]]` case (US-RR-14-8) |
//!
//! # Citations
//!
//! ```toml
//! [[citation]]                    # optional; required of a standard test-vector fixture
//! kind = "standard"               # standard | legacy-test | legacy-algorithm
//! doc = "ISO 14230-2"
//! edition = "1999 (first edition, 1999-03-15)"
//! section = "4.5.1, Table 3"
//! ```
//!
//! A fixture may name the documents its values rest on in top-level
//! `[[citation]]` tables ([`Citation`], US-RR-14-1). The validator rule
//! `citation` requires one of a standard test-vector fixture and checks each
//! ([`validate`]).

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use serde::Deserialize;
use serde::de::{self, Deserializer, Visitor};

use crate::fixtures::{self, Fixture, FixtureError, Tier};

pub mod validate;

/// A loaded schema-1 KAT fixture.
pub type KatFixture = Fixture<Kat>;

/// Loads and parses the KAT fixture at `path`.
///
/// # Errors
///
/// As [`fixtures::load_kat`]; a fixture that breaks the schema is
/// [`FixtureError::Shape`] with a message naming the offending key.
pub fn load(path: impl AsRef<Path>) -> Result<KatFixture, FixtureError> {
    fixtures::load_kat(path)
}

/// [`load`] for a path relative to `tier`'s root.
///
/// # Errors
///
/// As [`fixtures::load_kat_in`].
pub fn load_in(tier: Tier, relative: impl AsRef<Path>) -> Result<KatFixture, FixtureError> {
    fixtures::load_kat_in(tier, relative)
}

/// Parses KAT fixture text; `path` only names the fixture in errors.
///
/// # Errors
///
/// As [`fixtures::parse_kat`].
pub fn parse(path: impl Into<PathBuf>, text: &str) -> Result<KatFixture, FixtureError> {
    fixtures::parse_kat(path, text)
}

/// One KAT fixture (every top-level key except `schema`).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(try_from = "RawKat")]
pub struct Kat {
    /// One-line description of what the fixture pins down.
    pub title: String,
    /// Optional free-text notes for reviewers.
    pub notes: Option<String>,
    /// The bytes the cases are evaluated against; `None` exactly for an
    /// expression, unit, timing or codec fixture, which has none.
    pub input: Option<Input>,
    /// The definition and expected answers.
    pub kind: KatKind,
    /// Why expected values changed, oldest first (empty for a fixture whose
    /// values never changed).
    pub changelog: Vec<ChangelogEntry>,
    /// The documents the fixture's expected values rest on (`[[citation]]`,
    /// US-RR-14-1); empty when it cites none. Required of a standard
    /// test-vector fixture (validator rule `citation`).
    pub citations: Vec<Citation>,
}

/// The six fixture shapes.
#[derive(Debug, Clone, PartialEq)]
pub enum KatKind {
    /// `[[decode]]` cases: bytes → raw value → real value.
    Decode {
        /// How to read the input.
        definition: Definition,
        /// At least one case.
        cases: Vec<DecodeCase>,
        /// The conversion's display range, if the fixture pins it.
        range: Option<RangeCase>,
    },
    /// `[[checksum]]` cases: checksum state before and after a fix.
    Checksum {
        /// Optional definition (e.g. the ROM definition carrying the spec).
        definition: Option<Definition>,
        /// At least one case.
        cases: Vec<ChecksumCase>,
    },
    /// `[[expr]]` cases: formula and `x` → value. No input, no definition.
    Expr {
        /// At least one case.
        cases: Vec<ExprCase>,
    },
    /// `[[unit]]` cases: a value in one unit → the value in another. No
    /// input, no definition.
    Unit {
        /// At least one case.
        cases: Vec<UnitCase>,
    },
    /// `[[timing]]` cases: a named timing parameter → its duration. No
    /// input, no definition.
    Timing {
        /// At least one case.
        cases: Vec<TimingCase>,
    },
    /// `[[codec]]` cases: bytes in, an operation and its parameters → bytes
    /// out. No input, no definition.
    Codec {
        /// At least one case.
        cases: Vec<CodecCase>,
    },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawKat {
    title: String,
    notes: Option<String>,
    definition: Option<Definition>,
    input: Option<Input>,
    decode: Option<Vec<DecodeCase>>,
    checksum: Option<Vec<ChecksumCase>>,
    expr: Option<Vec<ExprCase>>,
    unit: Option<Vec<UnitCase>>,
    timing: Option<Vec<TimingCase>>,
    codec: Option<Vec<CodecCase>>,
    range: Option<RangeCase>,
    #[serde(default)]
    changelog: Vec<ChangelogEntry>,
    #[serde(default)]
    citation: Vec<Citation>,
}

impl TryFrom<RawKat> for Kat {
    type Error = String;

    fn try_from(mut raw: RawKat) -> Result<Self, Self::Error> {
        let shapes = usize::from(raw.decode.is_some())
            + usize::from(raw.checksum.is_some())
            + usize::from(raw.expr.is_some())
            + usize::from(raw.unit.is_some())
            + usize::from(raw.timing.is_some())
            + usize::from(raw.codec.is_some());
        if shapes > 1 {
            return Err(
                "fixture has more than one of [[decode]], [[checksum]], [[expr]], [[unit]], \
                 [[timing]] and [[codec]] cases; a fixture is one shape"
                    .to_owned(),
            );
        }
        if let Some(cases) = raw.codec.take() {
            if raw.definition.is_some() || raw.input.is_some() {
                return Err(
                    "a [[codec]] fixture has no [definition] or [input]: each case carries \
                     its own `input` and `output` bytes"
                        .to_owned(),
                );
            }
            if raw.range.is_some() {
                return Err(
                    "[range] is the display range of a decode fixture's conversion; \
                     only a fixture with [[decode]] cases has one"
                        .to_owned(),
                );
            }
            if cases.is_empty() {
                return Err("`codec` is empty: add at least one [[codec]] case".to_owned());
            }
            return Ok(Self {
                title: raw.title,
                notes: raw.notes,
                input: None,
                kind: KatKind::Codec { cases },
                changelog: raw.changelog,
                citations: raw.citation,
            });
        }
        if let Some(cases) = raw.timing.take() {
            if raw.definition.is_some() || raw.input.is_some() {
                return Err(
                    "a [[timing]] fixture has no [definition] or [input]: each case's `name` \
                     and value are the whole answer"
                        .to_owned(),
                );
            }
            if raw.range.is_some() {
                return Err(
                    "[range] is the display range of a decode fixture's conversion; \
                     only a fixture with [[decode]] cases has one"
                        .to_owned(),
                );
            }
            if cases.is_empty() {
                return Err("`timing` is empty: add at least one [[timing]] case".to_owned());
            }
            return Ok(Self {
                title: raw.title,
                notes: raw.notes,
                input: None,
                kind: KatKind::Timing { cases },
                changelog: raw.changelog,
                citations: raw.citation,
            });
        }
        if let Some(cases) = raw.unit.take() {
            if raw.definition.is_some() || raw.input.is_some() {
                return Err(
                    "a [[unit]] fixture has no [definition] or [input]: each case's `from`, \
                     `to` and `value` are the whole question"
                        .to_owned(),
                );
            }
            if raw.range.is_some() {
                return Err(
                    "[range] is the display range of a decode fixture's conversion; \
                     only a fixture with [[decode]] cases has one"
                        .to_owned(),
                );
            }
            if cases.is_empty() {
                return Err("`unit` is empty: add at least one [[unit]] case".to_owned());
            }
            return Ok(Self {
                title: raw.title,
                notes: raw.notes,
                input: None,
                kind: KatKind::Unit { cases },
                changelog: raw.changelog,
                citations: raw.citation,
            });
        }
        if raw.range.is_some() && raw.decode.is_none() {
            return Err(
                "[range] is the display range of a decode fixture's conversion; \
                 only a fixture with [[decode]] cases has one"
                    .to_owned(),
            );
        }
        let kind = match (raw.decode, raw.checksum, raw.expr) {
            (Some(cases), _, _) => {
                let Some(definition) = raw.definition else {
                    return Err(
                        "a [[decode]] fixture needs a [definition] (encoding, snippet or reference)"
                            .to_owned(),
                    );
                };
                if cases.is_empty() {
                    return Err("`decode` is empty: add at least one [[decode]] case".to_owned());
                }
                KatKind::Decode {
                    definition,
                    cases,
                    range: raw.range,
                }
            }
            (None, Some(cases), _) => {
                if cases.is_empty() {
                    return Err(
                        "`checksum` is empty: add at least one [[checksum]] case".to_owned()
                    );
                }
                KatKind::Checksum {
                    definition: raw.definition,
                    cases,
                }
            }
            (None, None, Some(cases)) => {
                if raw.definition.is_some() || raw.input.is_some() {
                    return Err(
                        "an [[expr]] fixture has no [definition] or [input]: each case's \
                         `formula` and `x` are the whole question"
                            .to_owned(),
                    );
                }
                if cases.is_empty() {
                    return Err("`expr` is empty: add at least one [[expr]] case".to_owned());
                }
                KatKind::Expr { cases }
            }
            (None, None, None) => {
                return Err(
                    "fixture has no cases: add [[decode]], [[checksum]], [[expr]], [[unit]], \
                     [[timing]] or [[codec]]"
                        .to_owned(),
                );
            }
        };
        if !matches!(kind, KatKind::Expr { .. }) && raw.input.is_none() {
            return Err(
                "missing field `input`: a decode or checksum fixture needs an [input] \
                 (`hex` or `rom`)"
                    .to_owned(),
            );
        }
        Ok(Self {
            title: raw.title,
            notes: raw.notes,
            input: raw.input,
            kind,
            changelog: raw.changelog,
            citations: raw.citation,
        })
    }
}

// ---------------------------------------------------------------------------
// Definition
// ---------------------------------------------------------------------------

/// `[definition]`: exactly one of `encoding` (+ optional `conversion`),
/// `snippet` or `reference`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(try_from = "RawDefinition")]
pub enum Definition {
    /// `[definition.encoding]` (+ optional `[definition.conversion]`).
    Inline {
        /// Storage, endian, layout and bitfield.
        encoding: Encoding,
        /// Raw → real conversion, if the fixture has real values.
        conversion: Option<Conversion>,
    },
    /// `snippet = { format, text }`: a fragment of a definition file.
    Snippet(Snippet),
    /// `reference = { tier, path, item }`: a definition file in a tier.
    Reference(DefinitionRef),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawDefinition {
    encoding: Option<Encoding>,
    conversion: Option<Conversion>,
    snippet: Option<Snippet>,
    reference: Option<DefinitionRef>,
}

impl TryFrom<RawDefinition> for Definition {
    type Error = String;

    fn try_from(raw: RawDefinition) -> Result<Self, Self::Error> {
        match (raw.encoding, raw.snippet, raw.reference) {
            (Some(encoding), None, None) => Ok(Self::Inline {
                encoding,
                conversion: raw.conversion,
            }),
            (None, Some(snippet), None) if raw.conversion.is_none() => Ok(Self::Snippet(snippet)),
            (None, None, Some(reference)) if raw.conversion.is_none() => {
                Ok(Self::Reference(reference))
            }
            (None, None, None) if raw.conversion.is_some() => Err(
                "[definition] has `conversion` but no `encoding`; `conversion` goes with an inline `encoding`"
                    .to_owned(),
            ),
            (None, None, None) => Err(
                "[definition] needs exactly one of `encoding`, `snippet` or `reference`".to_owned(),
            ),
            (None, Some(_), None) | (None, None, Some(_)) => Err(
                "[definition] `conversion` is only allowed with an inline `encoding`".to_owned(),
            ),
            _ => Err(
                "[definition] has more than one of `encoding`, `snippet` and `reference`; use exactly one"
                    .to_owned(),
            ),
        }
    }
}

/// `[definition.encoding]`: how bytes become a raw value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Encoding {
    /// Storage type.
    pub storage: Storage,
    /// Byte order. Required even for one-byte storage, so every fixture is
    /// explicit.
    pub endian: Endian,
    /// Cell layout; [`Layout::Default`] if omitted.
    #[serde(default)]
    pub layout: Layout,
    /// Optional bitfield extraction applied to the stored value.
    pub bitfield: Option<Bitfield>,
}

/// Storage types a KAT fixture can express (names as written in TOML).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Storage {
    /// Unsigned 8-bit.
    U8,
    /// Unsigned 16-bit.
    U16,
    /// Unsigned 20-bit immediate (SH-2A `MOVI20`).
    U20,
    /// Unsigned 24-bit.
    U24,
    /// Unsigned 32-bit.
    U32,
    /// Unsigned 64-bit.
    U64,
    /// Signed 8-bit.
    I8,
    /// Signed 16-bit.
    I16,
    /// Signed 24-bit.
    I24,
    /// Signed 32-bit.
    I32,
    /// Signed 64-bit.
    I64,
    /// IEEE 754 binary32.
    F32,
    /// IEEE 754 binary64.
    F64,
}

impl Storage {
    /// Every storage type, in declaration order (for coverage checks such as
    /// "the seed fixtures cover every storage type").
    pub const ALL: [Self; 13] = [
        Self::U8,
        Self::U16,
        Self::U20,
        Self::U24,
        Self::U32,
        Self::U64,
        Self::I8,
        Self::I16,
        Self::I24,
        Self::I32,
        Self::I64,
        Self::F32,
        Self::F64,
    ];

    /// The TOML spelling (`"u8"`, `"i24"`, `"f64"`, ...).
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::U8 => "u8",
            Self::U16 => "u16",
            Self::U20 => "u20",
            Self::U24 => "u24",
            Self::U32 => "u32",
            Self::U64 => "u64",
            Self::I8 => "i8",
            Self::I16 => "i16",
            Self::I24 => "i24",
            Self::I32 => "i32",
            Self::I64 => "i64",
            Self::F32 => "f32",
            Self::F64 => "f64",
        }
    }

    /// Whether this is a floating-point storage type.
    #[must_use]
    pub const fn is_float(self) -> bool {
        matches!(self, Self::F32 | Self::F64)
    }
}

impl fmt::Display for Storage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// Byte order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Endian {
    /// Most significant byte first.
    Big,
    /// Least significant byte first.
    Little,
}

/// Cell layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Layout {
    /// Plain storage.
    #[default]
    Default,
    /// Bosch "subtract" layout (`"bosch-subtract"`).
    BoschSubtract,
}

/// `bitfield = { mask, shift }`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(try_from = "RawBitfield")]
pub struct Bitfield {
    /// Non-zero mask applied to the stored value.
    pub mask: u64,
    /// Right shift after masking; `None` means the mask's trailing zeros.
    pub shift: Option<u32>,
}

impl Bitfield {
    /// The effective shift: the explicit `shift`, else the mask's trailing
    /// zeros.
    #[must_use]
    pub const fn effective_shift(self) -> u32 {
        match self.shift {
            Some(shift) => shift,
            None => self.mask.trailing_zeros(),
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawBitfield {
    #[serde(deserialize_with = "flex_u64")]
    mask: u64,
    shift: Option<u32>,
}

impl TryFrom<RawBitfield> for Bitfield {
    type Error = String;

    fn try_from(raw: RawBitfield) -> Result<Self, Self::Error> {
        if raw.mask == 0 {
            return Err("bitfield `mask` must be non-zero".to_owned());
        }
        if raw.shift.is_some_and(|shift| shift >= 64) {
            return Err("bitfield `shift` must be below 64".to_owned());
        }
        Ok(Self {
            mask: raw.mask,
            shift: raw.shift,
        })
    }
}

/// `[definition.conversion]`: raw → real.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Conversion {
    /// Expression from raw `x` to real.
    pub to_real: String,
    /// Optional inverse expression from real `x` to raw.
    pub to_raw: Option<String>,
    /// Optional units label.
    pub units: Option<String>,
}

/// `snippet = { format, text }`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Snippet {
    /// What kind of definition text this is.
    pub format: SnippetFormat,
    /// The text, verbatim.
    pub text: String,
}

/// Definition-file formats a snippet can be written in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SnippetFormat {
    /// RomRaider ECU-definition XML.
    RomraiderEcuXml,
    /// RomRaider logger-definition XML.
    RomraiderLoggerXml,
    /// TunerPro XDF.
    Xdf,
    /// ECUFlash XML.
    EcuflashXml,
    /// RomRaider-RS TOML profile.
    TomlProfile,
}

/// `reference = { tier, path, item }`: a definition file in a fixture tier.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DefinitionRef {
    /// Which tier the file lives in.
    pub tier: Tier,
    /// Path relative to the tier root.
    pub path: PathBuf,
    /// Optional table / scaling / parameter name within the file.
    pub item: Option<String>,
}

// ---------------------------------------------------------------------------
// Input
// ---------------------------------------------------------------------------

/// `[input]`: exactly one of `hex` or `rom`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "RawInput")]
pub enum Input {
    /// `hex = "..."`: the bytes, inline.
    Hex(HexBytes),
    /// `rom = { .. }`: a slice of a ROM file in a fixture tier.
    Rom(RomRef),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawInput {
    hex: Option<HexBytes>,
    rom: Option<RomRef>,
}

impl TryFrom<RawInput> for Input {
    type Error = String;

    fn try_from(raw: RawInput) -> Result<Self, Self::Error> {
        match (raw.hex, raw.rom) {
            (Some(hex), None) => Ok(Self::Hex(hex)),
            (None, Some(rom)) => Ok(Self::Rom(rom)),
            (Some(_), Some(_)) => {
                Err("[input] has both `hex` and `rom`; use exactly one".to_owned())
            }
            (None, None) => Err("[input] needs exactly one of `hex` or `rom`".to_owned()),
        }
    }
}

/// `rom = { tier, path, sha256, offset, length }`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RomRef {
    /// Which tier the ROM lives in.
    pub tier: Tier,
    /// Path relative to the tier root.
    pub path: PathBuf,
    /// SHA-256 of the whole ROM file, so a fixture never silently runs
    /// against a different image.
    pub sha256: Sha256,
    /// Byte offset of the input slice within the ROM (default 0).
    #[serde(default, deserialize_with = "flex_u64")]
    pub offset: u64,
    /// Length of the input slice; `None` means to the end of the ROM.
    #[serde(default, deserialize_with = "flex_u64_opt")]
    pub length: Option<u64>,
}

/// A SHA-256 digest written as 64 hex digits (stored lowercase).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Sha256(String);

impl Sha256 {
    /// The digest as 64 lowercase hex digits.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for Sha256 {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        if text.len() == 64 && text.bytes().all(|b| b.is_ascii_hexdigit()) {
            Ok(Self(text.to_ascii_lowercase()))
        } else {
            Err(format!(
                "invalid sha256 {text:?}: expected exactly 64 hex digits"
            ))
        }
    }
}

impl<'de> Deserialize<'de> for Sha256 {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        text.parse().map_err(de::Error::custom)
    }
}

// ---------------------------------------------------------------------------
// Hex bytes
// ---------------------------------------------------------------------------

/// Bytes written as hex digits, e.g. `"01 F4 ff38"`.
///
/// ASCII whitespace between digits is ignored (so bytes can be grouped and
/// split across lines); anything else that is not a hex digit, an odd number
/// of digits, or no digits at all is an error. No `0x` prefix.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct HexBytes(pub Vec<u8>);

impl HexBytes {
    /// The bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

/// Why a hex string was rejected.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum HexError {
    /// No hex digits at all.
    #[error("invalid hex: no bytes")]
    Empty,
    /// A character that is neither a hex digit nor whitespace.
    #[error("invalid hex: {found:?} at character {index} is not a hex digit")]
    InvalidChar {
        /// The offending character.
        found: char,
        /// Its index (in characters) in the string.
        index: usize,
    },
    /// An odd number of hex digits (half a byte left over).
    #[error("invalid hex: odd number of hex digits ({digits})")]
    OddDigits {
        /// How many digits were found.
        digits: usize,
    },
}

impl FromStr for HexBytes {
    type Err = HexError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let mut nibbles = Vec::with_capacity(text.len());
        for (index, found) in text.chars().enumerate() {
            if found.is_ascii_whitespace() {
                continue;
            }
            match found.to_digit(16) {
                // `to_digit(16)` is at most 15, so this always fits.
                Some(nibble) => nibbles.push(u8::try_from(nibble).unwrap_or(0)),
                None => return Err(HexError::InvalidChar { found, index }),
            }
        }
        if nibbles.is_empty() {
            return Err(HexError::Empty);
        }
        if nibbles.len() % 2 != 0 {
            return Err(HexError::OddDigits {
                digits: nibbles.len(),
            });
        }
        Ok(Self(
            nibbles
                .chunks_exact(2)
                .map(|pair| (pair[0] << 4) | pair[1])
                .collect(),
        ))
    }
}

impl<'de> Deserialize<'de> for HexBytes {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        text.parse().map_err(de::Error::custom)
    }
}

// ---------------------------------------------------------------------------
// Cases
// ---------------------------------------------------------------------------

/// One `[[decode]]` case.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DecodeCase {
    /// Byte offset of the cell within the input (default 0).
    #[serde(default, deserialize_with = "flex_u64")]
    pub offset: u64,
    /// Expected raw value.
    pub raw: RawValue,
    /// Expected real value, if the definition has a conversion.
    pub real: Option<f64>,
    /// Absolute tolerance on `real` (exact if omitted).
    pub tolerance: Option<f64>,
    /// Where `raw` and `real` came from. Required.
    pub source: Source,
    /// Optional free-text note.
    pub note: Option<String>,
}

/// `[range]`: the display range of a decode fixture's conversion over every
/// raw value its cell can hold (the storage's range, or the bitfield's), as
/// a display shows it (US-RR-24-2, PROJECT.md MF-13).
///
/// The extremes are over the raw values themselves, not `f(lowest raw)` and
/// `f(highest raw)`: for a decreasing scaling `real_min` is at the top of
/// the raw range. Each `raw_at_*` is the lowest raw value giving that real
/// value. Raw values with no real value (a division by zero) are left out.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RangeCase {
    /// The smallest real value.
    pub real_min: f64,
    /// The lowest raw value whose real value is `real_min`.
    pub raw_at_min: RawValue,
    /// The largest real value.
    pub real_max: f64,
    /// The lowest raw value whose real value is `real_max`.
    pub raw_at_max: RawValue,
    /// How the real value moves as the raw value increases.
    pub monotonicity: Monotonicity,
    /// Absolute tolerance on `real_min` and `real_max` (exact if omitted).
    pub tolerance: Option<f64>,
    /// Where the range came from. Required.
    pub source: Source,
    /// Optional free-text note.
    pub note: Option<String>,
}

/// `monotonicity` of a [`RangeCase`], over the raw values that have a real
/// value. Not strict: neighbouring raw values may share a real value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Monotonicity {
    /// Never falls, and rises somewhere.
    Increasing,
    /// Never rises, and falls somewhere.
    Decreasing,
    /// Every raw value gives the same real value.
    Constant,
    /// Both rises and falls.
    NonMonotonic,
}

impl Monotonicity {
    /// The TOML spelling.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Increasing => "increasing",
            Self::Decreasing => "decreasing",
            Self::Constant => "constant",
            Self::NonMonotonic => "non-monotonic",
        }
    }
}

/// One `[[expr]]` case: a formula evaluated at `x` (and, for a formula with
/// parameter references, at `params`).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(try_from = "RawExprCase")]
pub struct ExprCase {
    /// The formula: inline text (`formula = "..."`), or a formula of the
    /// definition corpus named by its hash (`formula_ref = { .. }`).
    pub formula: Formula,
    /// The value of `x` (default 0).
    pub x: f64,
    /// The value of each parameter reference, by its id (`P9` for `[P9:km/h]`
    /// or a bare `P9`); empty for a formula in `x` only.
    pub params: BTreeMap<String, f64>,
    /// The value the formula evaluates to. May be `inf`, `-inf` or `nan`
    /// (the documented result of, say, `1/0` or `(-8)^(1/3)`).
    pub value: f64,
    /// Absolute tolerance on a finite `value` (exact if omitted).
    pub tolerance: Option<f64>,
    /// Where `value` came from. Required.
    pub source: Source,
    /// Optional free-text note.
    pub note: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawExprCase {
    formula: Option<String>,
    formula_ref: Option<FormulaRef>,
    #[serde(default)]
    x: f64,
    #[serde(default)]
    params: BTreeMap<String, f64>,
    value: f64,
    tolerance: Option<f64>,
    source: Source,
    note: Option<String>,
}

impl TryFrom<RawExprCase> for ExprCase {
    type Error = String;

    fn try_from(raw: RawExprCase) -> Result<Self, Self::Error> {
        let formula = match (raw.formula, raw.formula_ref) {
            (Some(text), None) => Formula::Text(text),
            (None, Some(reference)) => Formula::Ref(reference),
            (Some(_), Some(_)) => {
                return Err(
                    "[[expr]] case has both `formula` and `formula_ref`; use exactly one"
                        .to_owned(),
                );
            }
            (None, None) => {
                return Err(
                    "[[expr]] case needs `formula` (inline text) or `formula_ref` (a corpus formula by hash)"
                        .to_owned(),
                );
            }
        };
        Ok(Self {
            formula,
            x: raw.x,
            params: raw.params,
            value: raw.value,
            tolerance: raw.tolerance,
            source: raw.source,
            note: raw.note,
        })
    }
}

/// The formula of an `[[expr]]` case.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Formula {
    /// `formula = "..."`: the text, inline. Only for formulas we may publish:
    /// written for the fixture, or from a published standard.
    Text(String),
    /// `formula_ref = { .. }`: a formula of the definition corpus, which has
    /// no licence, so its text stays in the fetched corpus (D-028) and the
    /// fixture names it by hash.
    Ref(FormulaRef),
}

impl Formula {
    /// The inline text, or `None` for a reference.
    #[must_use]
    pub fn text(&self) -> Option<&str> {
        match self {
            Self::Text(text) => Some(text),
            Self::Ref(_) => None,
        }
    }

    /// The reference, or `None` for inline text.
    #[must_use]
    pub const fn reference(&self) -> Option<&FormulaRef> {
        match self {
            Self::Text(_) => None,
            Self::Ref(reference) => Some(reference),
        }
    }
}

impl fmt::Display for Formula {
    /// The text, or `sha256:` and the first 12 hex digits of a reference
    /// (never the corpus text, which a fixture does not have).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Text(text) => f.write_str(text),
            Self::Ref(reference) => {
                let hex = reference.sha256.as_str();
                write!(f, "sha256:{}", hex.get(..12).unwrap_or(hex))
            }
        }
    }
}

/// `formula_ref = { tier, sha256, path, xpath }`: one formula of the
/// definition corpus (`rr_testkit::corpus::formulas`).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FormulaRef {
    /// The tier holding the definitions: `corpus` (the public, fetched
    /// definition corpus) or `private`. Never `public`: a formula we may
    /// publish goes inline as `formula`.
    pub tier: FormulaTier,
    /// SHA-256 of the formula's UTF-8 text: what identifies it.
    pub sha256: Sha256,
    /// A file it occurs in, relative to the tier root
    /// (`defs/romraider/ecu/standard/ecu_defs.xml`).
    pub path: PathBuf,
    /// Where in that file: the positional XPath of the attribute
    /// (`/roms[1]/rom[1]/table[3]/scaling[1]/@expression`).
    pub xpath: String,
}

/// The tiers a [`FormulaRef`] may name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FormulaTier {
    /// [`Tier::Corpus`].
    Corpus,
    /// [`Tier::Private`].
    Private,
}

impl From<FormulaTier> for Tier {
    fn from(tier: FormulaTier) -> Self {
        match tier {
            FormulaTier::Corpus => Self::Corpus,
            FormulaTier::Private => Self::Private,
        }
    }
}

/// One `[[unit]]` case: `value` in unit `from` is `expected` in unit `to`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(try_from = "RawUnitCase")]
pub struct UnitCase {
    /// The unit `value` is in: an id of rr-core's unit registry.
    pub from: String,
    /// The unit `expected` is in: an id of rr-core's unit registry.
    pub to: String,
    /// The value converted. Finite.
    pub value: f64,
    /// `value` in `to`. Finite.
    pub expected: f64,
    /// Absolute tolerance on `expected` (exact if omitted).
    pub tolerance: Option<f64>,
    /// The fuel of an air/fuel ratio ↔ lambda case: an id of rr-core's
    /// `units::FUELS`. `None` for every other case.
    pub fuel: Option<String>,
    /// Where `expected` came from. Required.
    pub source: Source,
    /// Optional free-text note.
    pub note: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawUnitCase {
    from: String,
    to: String,
    value: f64,
    expected: f64,
    tolerance: Option<f64>,
    fuel: Option<String>,
    source: Source,
    note: Option<String>,
}

impl TryFrom<RawUnitCase> for UnitCase {
    type Error = String;

    fn try_from(raw: RawUnitCase) -> Result<Self, Self::Error> {
        if raw.from.trim().is_empty() || raw.to.trim().is_empty() {
            return Err("[[unit]] case needs a non-empty `from` and `to` unit id".to_owned());
        }
        if raw.fuel.as_ref().is_some_and(|f| f.trim().is_empty()) {
            return Err("[[unit]] case `fuel` is empty: name a fuel id or leave it out".to_owned());
        }
        if !raw.value.is_finite() || !raw.expected.is_finite() {
            return Err(format!(
                "[[unit]] case `value` ({}) and `expected` ({}) must be finite: a unit \
                 conversion of a finite value is finite",
                raw.value, raw.expected
            ));
        }
        Ok(Self {
            from: raw.from,
            to: raw.to,
            value: raw.value,
            expected: raw.expected,
            tolerance: raw.tolerance,
            fuel: raw.fuel,
            source: raw.source,
            note: raw.note,
        })
    }
}

/// The unit a `[[timing]]` case's value is written in: its `ms` or `us` key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TimeUnit {
    /// `ms`: whole milliseconds.
    Millis,
    /// `us`: whole microseconds.
    Micros,
}

impl TimeUnit {
    /// The fixture key, `ms` or `us`.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Millis => "ms",
            Self::Micros => "us",
        }
    }
}

/// One `[[timing]]` case: the timing parameter `name` is `amount` `unit`s.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(try_from = "RawTimingCase")]
pub struct TimingCase {
    /// The constant: its path under rr-core's `timing::iso`
    /// (`iso14230_2::P3_MIN`).
    pub name: String,
    /// The value, a whole number of `unit`s.
    pub amount: u64,
    /// Which key the value was written with.
    pub unit: TimeUnit,
    /// Where the value came from. Required.
    pub source: Source,
    /// Optional free-text note.
    pub note: Option<String>,
}

impl TimingCase {
    /// The value as a [`Duration`](std::time::Duration).
    #[must_use]
    pub const fn duration(&self) -> std::time::Duration {
        match self.unit {
            TimeUnit::Millis => std::time::Duration::from_millis(self.amount),
            TimeUnit::Micros => std::time::Duration::from_micros(self.amount),
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawTimingCase {
    name: String,
    ms: Option<u64>,
    us: Option<u64>,
    source: Source,
    note: Option<String>,
}

impl TryFrom<RawTimingCase> for TimingCase {
    type Error = String;

    fn try_from(raw: RawTimingCase) -> Result<Self, Self::Error> {
        if raw.name.trim().is_empty() || raw.name.chars().any(char::is_whitespace) {
            return Err(format!(
                "[[timing]] case `name` {:?} must be a constant path without spaces, \
                 like \"iso14230_2::P3_MIN\"",
                raw.name
            ));
        }
        let (amount, unit) = match (raw.ms, raw.us) {
            (Some(ms), None) => (ms, TimeUnit::Millis),
            (None, Some(us)) => (us, TimeUnit::Micros),
            _ => {
                return Err(format!(
                    "[[timing]] case {:?} needs exactly one of `ms` and `us`",
                    raw.name
                ));
            }
        };
        Ok(Self {
            name: raw.name,
            amount,
            unit,
            source: raw.source,
            note: raw.note,
        })
    }
}

/// One `[[codec]]` case: operation `op` with `params` turns `input` into
/// `output`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(try_from = "RawCodecCase")]
pub struct CodecCase {
    /// The transform (`ncs-encode`), named by the test that runs it.
    pub op: String,
    /// Operation-specific parameters (empty if omitted).
    pub params: toml::Table,
    /// The bytes in.
    pub input: HexBytes,
    /// The bytes out.
    pub output: HexBytes,
    /// Where `output` came from. Required.
    pub source: Source,
    /// Optional free-text note.
    pub note: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawCodecCase {
    op: String,
    #[serde(default)]
    params: toml::Table,
    input: HexBytes,
    output: HexBytes,
    source: Source,
    note: Option<String>,
}

impl TryFrom<RawCodecCase> for CodecCase {
    type Error = String;

    fn try_from(raw: RawCodecCase) -> Result<Self, Self::Error> {
        if raw.op.trim().is_empty() || raw.op.chars().any(char::is_whitespace) {
            return Err(format!(
                "[[codec]] case `op` {:?} must be an operation name without spaces, \
                 like \"ncs-encode\"",
                raw.op
            ));
        }
        Ok(Self {
            op: raw.op,
            params: raw.params,
            input: raw.input,
            output: raw.output,
            source: raw.source,
            note: raw.note,
        })
    }
}

/// An expected raw value: an integer (covering the whole `i64` and `u64`
/// ranges) or a float (for `f32`/`f64` storage).
///
/// TOML integers are `i64`, so a `u64` above `i64::MAX` is written as a
/// string: `raw = "0xFFFF_FFFF_FFFF_FFFF"`. Strings accept an optional `-`,
/// a `0x` / `0b` prefix, and `_` separators.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RawValue {
    /// An integer raw value.
    Int(i128),
    /// A floating-point raw value.
    Float(f64),
}

impl<'de> Deserialize<'de> for RawValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct RawVisitor;

        impl Visitor<'_> for RawVisitor {
            type Value = RawValue;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("an integer, a float, or an integer string such as \"0xFFFF\"")
            }

            fn visit_i64<E: de::Error>(self, v: i64) -> Result<RawValue, E> {
                Ok(RawValue::Int(i128::from(v)))
            }

            fn visit_u64<E: de::Error>(self, v: u64) -> Result<RawValue, E> {
                Ok(RawValue::Int(i128::from(v)))
            }

            fn visit_f64<E: de::Error>(self, v: f64) -> Result<RawValue, E> {
                Ok(RawValue::Float(v))
            }

            fn visit_str<E: de::Error>(self, v: &str) -> Result<RawValue, E> {
                parse_int(v)
                    .filter(|n| (i128::from(i64::MIN)..=i128::from(u64::MAX)).contains(n))
                    .map(RawValue::Int)
                    .ok_or_else(|| {
                        E::custom(format!(
                            "invalid raw value {v:?}: expected an integer in the i64/u64 range"
                        ))
                    })
            }
        }

        deserializer.deserialize_any(RawVisitor)
    }
}

/// One `[[checksum]]` case.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChecksumCase {
    /// Named checksum algorithm.
    pub algorithm: String,
    /// Algorithm-specific parameters (empty if omitted).
    #[serde(default)]
    pub params: toml::Table,
    /// Byte offset of the stored checksum within the input.
    #[serde(deserialize_with = "flex_u64")]
    pub offset: u64,
    /// The stored checksum and verify result for the input as given.
    pub before: ChecksumState,
    /// The stored checksum and verify result after fixing checksums.
    pub after: ChecksumState,
    /// Optional free-text note.
    pub note: Option<String>,
}

/// `before` / `after` of a [`ChecksumCase`].
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChecksumState {
    /// The stored checksum bytes.
    pub hex: HexBytes,
    /// Whether the checksum verifies.
    pub valid: bool,
    /// Where these expected values came from. Required.
    pub source: Source,
}

// ---------------------------------------------------------------------------
// Source
// ---------------------------------------------------------------------------

/// Where an expected value came from (`source = { kind = "...", .. }`).
///
/// Required on every expected value; there is deliberately no variant for
/// "the code under test" or "the Java app" (D-024).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Source {
    /// `hand-calc`: calculated by hand.
    HandCalc {
        /// The formula and intermediate values.
        working: String,
    },
    /// `standard`: a published standard document or reference vector.
    Standard {
        /// The document (e.g. "SAE J1979").
        doc: String,
        /// The section, table or vector within it.
        section: String,
    },
    /// `stock-rom`: an invariant of a stock ROM.
    StockRom {
        /// The ROM (ROM ID or file digest).
        rom: String,
        /// Optional detail.
        note: Option<String>,
    },
    /// `car-trace`: observed in a recorded car trace.
    CarTrace {
        /// The trace (file or identifier).
        trace: String,
        /// Optional detail.
        note: Option<String>,
    },
    /// `cross-source`: agreement between independent sources.
    CrossSource {
        /// The sources that agree.
        sources: Vec<String>,
    },
    /// `legacy-test`: literal data transcribed from a legacy test file
    /// (US-RR-14-8). Not a value the Java application computed: the test
    /// file's own literals, cited where they are. Only a `[[codec]]` case
    /// may have one (validator rule `legacy-test`).
    LegacyTest {
        /// The test file, relative to the repository root, under
        /// `legacy/src/test/`.
        file: String,
        /// The lines the data is on: comma-separated line numbers and
        /// `first-last` ranges, ascending (`"33,35,37,39-43"`).
        lines: String,
    },
}

impl Source {
    /// The TOML `kind` spelling.
    #[must_use]
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::HandCalc { .. } => "hand-calc",
            Self::Standard { .. } => "standard",
            Self::StockRom { .. } => "stock-rom",
            Self::CarTrace { .. } => "car-trace",
            Self::CrossSource { .. } => "cross-source",
            Self::LegacyTest { .. } => "legacy-test",
        }
    }
}

/// Parses a `legacy-test` source's `lines`: comma-separated line numbers
/// (from 1) and `first-last` ranges (`first <= last`), each starting after
/// the previous one ends, e.g. `"33,35,37,39-43"`. Whitespace around items
/// is ignored.
///
/// # Errors
///
/// A message naming the bad item, for an empty spec, a non-number, line 0,
/// a backwards range or items out of order.
pub fn parse_line_spec(spec: &str) -> Result<Vec<std::ops::RangeInclusive<u32>>, String> {
    let mut out: Vec<std::ops::RangeInclusive<u32>> = Vec::new();
    if spec.trim().is_empty() {
        return Err("`lines` is empty: cite the line numbers the data is on".to_owned());
    }
    for item in spec.split(',') {
        let item = item.trim();
        let number = |text: &str| {
            text.trim()
                .parse::<u32>()
                .ok()
                .filter(|&n| n > 0)
                .ok_or_else(|| {
                    format!("`lines` item {item:?} is not a line number (from 1) or a range")
                })
        };
        let range = match item.split_once('-') {
            Some((first, last)) => number(first)?..=number(last)?,
            None => {
                let line = number(item)?;
                line..=line
            }
        };
        if range.start() > range.end() {
            return Err(format!("`lines` range {item:?} runs backwards"));
        }
        if out.last().is_some_and(|prev| range.start() <= prev.end()) {
            return Err(format!(
                "`lines` item {item:?} does not come after the previous one: list lines in \
                 ascending order, without overlap"
            ));
        }
        out.push(range);
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Citation
// ---------------------------------------------------------------------------

/// One `[[citation]]` (US-RR-14-1): a document the fixture's expected values
/// rest on, as structured fields. A case's [`Source`] says where its value
/// came from (often a hand calculation); a citation says which document
/// defines what was calculated (the formula, the algorithm, the table).
///
/// Every standard test-vector fixture needs at least one, and the validator
/// rule `citation` checks each (see [`validate`] for what "vector fixture"
/// means and which exceptions exist).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Citation {
    /// `standard`: a published standard, or a published reference-vector
    /// catalogue.
    Standard {
        /// The document, without its edition (`"ISO 14230-2"`).
        doc: String,
        /// The edition: a year (`"1999 (first edition, 1999-03-15)"`) or a
        /// revision / version identifier (`"version 3"`).
        edition: String,
        /// The clause, table, figure or entry within it.
        section: String,
        /// Whether `edition` and `section` were read first-hand (default
        /// `true`). `false` is a flagged exception: allowed only in the
        /// fixtures the validator pins, and only with `via`.
        #[serde(default = "verified_default")]
        verified: bool,
        /// The secondary sources the values were taken from instead, when
        /// `verified = false` (each a citation with its own revision or
        /// digest).
        #[serde(default)]
        via: Vec<String>,
        /// Optional detail (why it is unverified, ...).
        note: Option<String>,
    },
    /// `legacy-test`: the legacy *test* file whose literal data the fixture
    /// transcribes (the NCS codec vectors, US-RR-14-8): `file` under
    /// `legacy/src/test/`, `lines` as [`parse_line_spec`].
    LegacyTest {
        /// The test file, relative to the repository root.
        file: String,
        /// The lines the data is on.
        lines: String,
    },
    /// `legacy-algorithm`: the legacy *application* code whose algorithm the
    /// fixture's hand-worked values model, for an algorithm no published
    /// standard defines (the checksum sums and XORs). It cites semantics,
    /// never values (D-024: those are worked by hand). A flagged exception:
    /// allowed only in the fixtures the validator pins.
    LegacyAlgorithm {
        /// The source file, relative to the repository root, under
        /// `legacy/src/main/`.
        file: String,
        /// The lines of the algorithm.
        lines: String,
        /// Why no standard is cited. Required.
        note: String,
    },
}

const fn verified_default() -> bool {
    true
}

impl Citation {
    /// The TOML `kind` spelling.
    #[must_use]
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::Standard { .. } => "standard",
            Self::LegacyTest { .. } => "legacy-test",
            Self::LegacyAlgorithm { .. } => "legacy-algorithm",
        }
    }
}

// ---------------------------------------------------------------------------
// Changelog
// ---------------------------------------------------------------------------

/// One `[[changelog]]` entry: why an expected value changed.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "RawChangelogEntry")]
pub struct ChangelogEntry {
    /// When (a TOML local date, `date = 2026-10-19`).
    pub date: toml::value::Date,
    /// Why the old value was wrong.
    pub reason: String,
    /// Optional author.
    pub author: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawChangelogEntry {
    date: toml::value::Datetime,
    reason: String,
    author: Option<String>,
}

impl TryFrom<RawChangelogEntry> for ChangelogEntry {
    type Error = String;

    fn try_from(raw: RawChangelogEntry) -> Result<Self, Self::Error> {
        match raw.date {
            toml::value::Datetime {
                date: Some(date),
                time: None,
                offset: None,
            } => Ok(Self {
                date,
                reason: raw.reason,
                author: raw.author,
            }),
            other => Err(format!(
                "changelog `date` must be a plain date such as 2026-10-19, got {other}"
            )),
        }
    }
}

// ---------------------------------------------------------------------------
// Number helpers
// ---------------------------------------------------------------------------

/// Parses `[-](0x|0b)?digits` with `_` separators.
fn parse_int(text: &str) -> Option<i128> {
    let text = text.trim();
    let (negative, body) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text),
    };
    let (radix, digits) =
        if let Some(rest) = body.strip_prefix("0x").or_else(|| body.strip_prefix("0X")) {
            (16, rest)
        } else if let Some(rest) = body.strip_prefix("0b").or_else(|| body.strip_prefix("0B")) {
            (2, rest)
        } else {
            (10, body)
        };
    let digits: String = digits.chars().filter(|&c| c != '_').collect();
    if digits.is_empty() || digits.starts_with(['+', '-']) {
        return None;
    }
    let magnitude = i128::from_str_radix(&digits, radix).ok()?;
    Some(if negative { -magnitude } else { magnitude })
}

/// A `u64` written as a non-negative TOML integer or as an integer string
/// (`"0xFFFF_FFFF_FFFF_FFFF"`), for values above `i64::MAX`.
fn flex_u64<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u64, D::Error> {
    struct U64Visitor;

    impl Visitor<'_> for U64Visitor {
        type Value = u64;

        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("a non-negative integer, or an integer string such as \"0xFFFF\"")
        }

        fn visit_i64<E: de::Error>(self, v: i64) -> Result<u64, E> {
            u64::try_from(v)
                .map_err(|_| E::custom(format!("expected a non-negative integer, got {v}")))
        }

        fn visit_u64<E: de::Error>(self, v: u64) -> Result<u64, E> {
            Ok(v)
        }

        fn visit_str<E: de::Error>(self, v: &str) -> Result<u64, E> {
            parse_int(v)
                .and_then(|n| u64::try_from(n).ok())
                .ok_or_else(|| E::custom(format!("invalid unsigned integer {v:?}")))
        }
    }

    deserializer.deserialize_any(U64Visitor)
}

/// [`flex_u64`] for an optional field.
fn flex_u64_opt<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<u64>, D::Error> {
    flex_u64(deserializer).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_accepts_grouping_and_case() {
        assert_eq!(
            "01 f4\n FF38".parse::<HexBytes>().unwrap().as_bytes(),
            [0x01, 0xF4, 0xFF, 0x38]
        );
    }

    #[test]
    fn hex_rejects_bad_input() {
        assert_eq!("".parse::<HexBytes>(), Err(HexError::Empty));
        assert_eq!("  ".parse::<HexBytes>(), Err(HexError::Empty));
        assert_eq!(
            "0x01".parse::<HexBytes>(),
            Err(HexError::InvalidChar {
                found: 'x',
                index: 1
            })
        );
        assert_eq!(
            "01 G2".parse::<HexBytes>(),
            Err(HexError::InvalidChar {
                found: 'G',
                index: 3
            })
        );
        assert_eq!(
            "012".parse::<HexBytes>(),
            Err(HexError::OddDigits { digits: 3 })
        );
    }

    #[test]
    fn parse_int_forms() {
        assert_eq!(parse_int("42"), Some(42));
        assert_eq!(parse_int("-0x80"), Some(-128));
        assert_eq!(parse_int("0b1010_0101"), Some(0xA5));
        assert_eq!(
            parse_int("0xFFFF_FFFF_FFFF_FFFF"),
            Some(i128::from(u64::MAX))
        );
        assert_eq!(parse_int(""), None);
        assert_eq!(parse_int("0x"), None);
        assert_eq!(parse_int("--1"), None);
        assert_eq!(parse_int("0x-1"), None);
        assert_eq!(parse_int("12z"), None);
    }

    #[test]
    fn sha256_shape() {
        let digest = "AB".repeat(32);
        assert_eq!(digest.parse::<Sha256>().unwrap().as_str(), "ab".repeat(32));
        assert!("ab".repeat(31).parse::<Sha256>().is_err());
        assert!("zz".repeat(32).parse::<Sha256>().is_err());
    }

    #[test]
    fn storage_names_round_trip_through_serde() {
        #[derive(Deserialize)]
        struct One {
            storage: Storage,
        }
        for storage in Storage::ALL {
            let one: One = toml::from_str(&format!("storage = \"{storage}\"")).unwrap();
            assert_eq!(one.storage, storage);
        }
        assert!(Storage::F32.is_float() && !Storage::U20.is_float());
    }

    #[test]
    fn bitfield_shift_defaults_to_trailing_zeros() {
        let explicit = Bitfield {
            mask: 0x0FF0,
            shift: Some(2),
        };
        let implied = Bitfield {
            mask: 0x0FF0,
            shift: None,
        };
        assert_eq!(explicit.effective_shift(), 2);
        assert_eq!(implied.effective_shift(), 4);
    }
}
