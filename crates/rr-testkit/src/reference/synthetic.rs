//! Synthetic ROM generator (US-RR-10-6, PROJECT.md T2(b), D-024, EC-01/06/07).
//!
//! Builds a ROM image from known values, so public CI has real ROM-shaped
//! test data without committing a stock ROM, and emits a **manifest** of the
//! raw value every cell must decode to. The values are written with the
//! naive reference codec ([`super`]), never with rr-core: the manifest is the
//! cross-check *for* rr-core's Encoding, so it must not come from it. This
//! module lives under `reference/`, so `cargo xtask check-reference-isolation`
//! covers it: nothing here may name rr-core (or any workspace crate), or reach
//! a module that does.
//!
//! # Input: a [`Plan`]
//!
//! A plan is a ROM size, a fill byte and an ordered list of [`TablePlan`]s.
//! Each table has an address, an [`Encoding`] (storage, endian, layout,
//! bitfield), [`Dims`] (`cols x rows` cells, row-major, stored consecutively)
//! and [`Values`]: an explicit list, or a generator ([`Values::Extremes`],
//! [`Values::Specials`]). [`standard_plan`] is the plan public CI uses: the
//! min and max of every storage type in both byte orders, the special values
//! (NaN quiet and signalling with distinct payloads, ±Inf, ±0, min/max
//! subnormal, ...), bitfield and Bosch-subtract tables, overlapping tables
//! and a table that ends exactly at the end of the ROM.
//!
//! The plan is a Rust value rather than a parsed file: it needs no parser
//! (and so no parser bugs) and plans are reviewed as code. The manifest's
//! table entries carry everything a definition would (address, storage,
//! endian, layout, bitfield, dims), so no separate definition file is
//! emitted; when rr-defs exists, a RomRaider ECU XML emitter can be built
//! from the manifest without changing the generator.
//!
//! # Output: [`generate`] -> ROM bytes and a [`Manifest`]
//!
//! 1. The ROM starts as `size` copies of the fill byte, with the synthetic
//!    [`MARKER`] at offset 0. No table may touch the marker, so every
//!    generated ROM is recognisable as synthetic (the planned ROM-leak guard,
//!    US-RR-13, can rely on it), and the manifest says `synthetic = true`.
//! 2. **Overlap rule: tables are written in plan order, and the last write
//!    wins.** A later table's write replaces the bytes it owns; a bitfield
//!    table owns only its mask bits (read-modify-write), and a `u20` cell
//!    keeps its opcode nibble, exactly as [`super::encode_table`] does.
//! 3. After every table is written, every table is decoded from the
//!    **final** ROM with [`super::decode_table`]. That decoded value is the
//!    manifest's expected `raw` value for the cell, so the manifest always
//!    agrees with the bytes, overlaps included. A cell whose final value is
//!    not bit-for-bit what the plan wrote is marked `clobbered = true` and
//!    also records the `planned` value; each table lists the later tables
//!    whose bytes overlap it (`overlapped_by`), whether or not they changed
//!    a value (disjoint bitfields share bytes without clobbering).
//!
//! Plans are checked, not trusted: a table that does not fit the ROM, that
//! touches the marker, that has no cells or the wrong number of values, a
//! duplicate or malformed name, or a value the codec rejects is a
//! [`SyntheticError`], never a panic or a silently truncated ROM.
//!
//! # Determinism (US-RR-10-7 hashes the output on Linux and Windows)
//!
//! The output depends only on the plan: there are no hash maps (tables keep
//! plan order), no timestamps, no paths, and no float formatting. Floats are
//! written and recorded as their IEEE 754 bit patterns (`0x7fc00001`), never
//! as decimal text, and integers as decimal strings (so a `u64` above
//! `i64::MAX` or a Bosch value of `2^64` survives TOML, whose integers are
//! `i64`). [`Manifest::to_toml`] is hand-rendered with fixed key order. Float
//! values are only ever built with `f32::from_bits` / `f64::from_bits`, which
//! keep NaN payloads bit-exact on the 64-bit targets CI runs.
//!
//! # Manifest format (schema 1)
//!
//! ```toml
//! schema = 1
//! synthetic = true
//! plan = "standard"
//! rom_size = 4096
//! fill = 0xff
//! rom_fnv1a64 = "0x..."        # FNV-1a 64 of the ROM bytes
//! u20_bit19_sign_extends = false
//! marker = { offset = 0x0, length = 32, hex = "52 6f 6d ..." }
//!
//! [[table]]
//! name = "u16-be-extremes"
//! write_order = 0              # position in the plan
//! address = 0x40
//! end = 0x44                   # exclusive
//! storage = "u16"
//! endian = "big"
//! layout = "default"           # or "bosch-subtract"
//! bitfield = { mask = "0xff0", shift = 4 }   # only for bitfield tables
//! cols = 2
//! rows = 1
//! overlapped_by = []
//! cells = [
//!   { index = 0, offset = 0x40, raw = "0" },
//!   { index = 1, offset = 0x42, raw = "65535" },
//! ]
//! ```
//!
//! A cell's `raw` is a decimal string for integer storage, bitfields and
//! Bosch-subtract values, and a `0x`-prefixed lowercase hex bit pattern of
//! fixed width (8 or 16 digits) for `f32` / `f64`, which also carry a
//! `class` (`+qnan`, `-snan`, `+inf`, `-zero`, `+subnormal`, `+normal`, ...).
//! A clobbered cell adds `clobbered = true` and `planned` (same format).
//! `offset` is the cell's absolute ROM address. A table may also have a
//! `note`.

use std::fmt::{self, Write as _};

use super::{
    Bitfield, Encoding, Endian, Layout, Raw, ReferenceError, Storage, U20_BIT19_SIGN_EXTENDS,
    decode_table, encode_table,
};

/// The synthetic marker written at offset 0 of every generated ROM.
pub const MARKER: &[u8; 32] = b"RomRaider-RS SYNTHETIC ROM v1\0\0\0";

/// Bytes reserved for [`MARKER`] at the start of the ROM; no table may use
/// them.
pub const MARKER_LEN: usize = MARKER.len();

/// Largest ROM a plan may ask for (64 MiB; real ECU ROMs are a few MiB).
pub const MAX_ROM_SIZE: usize = 64 * 1024 * 1024;

/// Manifest schema version written by [`Manifest::to_toml`].
pub const MANIFEST_SCHEMA: u32 = 1;

/// Longest table or plan name.
pub const MAX_NAME_LEN: usize = 64;

/// The overlap rule, as recorded in the manifest.
pub const OVERLAP_RULE: &str = "tables are written in plan order and the last write wins; a bitfield table rewrites only its mask bits and a u20 cell keeps its opcode nibble; expected values are decoded from the final ROM";

// ---------------------------------------------------------------------------
// Plan
// ---------------------------------------------------------------------------

/// What to generate.
#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    /// Plan name, recorded in the manifest (ASCII letters, digits, `-`, `_`,
    /// `.`; at most [`MAX_NAME_LEN`]).
    pub name: String,
    /// ROM size in bytes ([`MARKER_LEN`]..=[`MAX_ROM_SIZE`]).
    pub size: usize,
    /// Byte every untouched ROM byte holds (`0xFF` is erased flash).
    pub fill: u8,
    /// Tables, written in this order (see the overlap rule in the module
    /// docs).
    pub tables: Vec<TablePlan>,
}

/// One table of a [`Plan`].
#[derive(Debug, Clone, PartialEq)]
pub struct TablePlan {
    /// Unique table name (same character rules as [`Plan::name`]).
    pub name: String,
    /// ROM address of the first cell.
    pub address: usize,
    /// How the cells are stored.
    pub encoding: Encoding,
    /// Cell grid; cells are stored row-major and consecutively.
    pub dims: Dims,
    /// The values to write.
    pub values: Values,
    /// Optional free-text note for the manifest.
    pub note: Option<String>,
}

impl TablePlan {
    /// A table with no note.
    #[must_use]
    pub fn new(
        name: impl Into<String>,
        address: usize,
        encoding: Encoding,
        dims: Dims,
        values: Values,
    ) -> Self {
        Self {
            name: name.into(),
            address,
            encoding,
            dims,
            values,
            note: None,
        }
    }

    /// The same table with a note.
    #[must_use]
    pub fn with_note(self, note: impl Into<String>) -> Self {
        Self {
            note: Some(note.into()),
            ..self
        }
    }
}

/// A table's cell grid: `cols x rows` cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Dims {
    /// Cells per row.
    pub cols: usize,
    /// Rows.
    pub rows: usize,
}

impl Dims {
    /// A single cell.
    #[must_use]
    pub const fn scalar() -> Self {
        Self { cols: 1, rows: 1 }
    }

    /// One row of `cols` cells.
    #[must_use]
    pub const fn row(cols: usize) -> Self {
        Self { cols, rows: 1 }
    }

    /// `cols x rows` cells.
    #[must_use]
    pub const fn grid(cols: usize, rows: usize) -> Self {
        Self { cols, rows }
    }

    /// Number of cells, or `None` if `cols * rows` overflows.
    #[must_use]
    pub const fn cells(self) -> Option<usize> {
        self.cols.checked_mul(self.rows)
    }
}

/// The values a table holds.
///
/// For the default layout a generator's sequence is the cell values. For the
/// Bosch-subtract layout a generator's sequence is the **stored** cells (so
/// the stored cells hit the storage's extremes), converted to the layout's
/// values (`value[i] = 2^(8W) - (s[i] + ... + s[N-1])`); an explicit list is
/// always the values themselves. A generator's sequence is repeated to fill
/// the table; a table with fewer cells than the sequence is an error, so no
/// generated value is silently dropped.
#[derive(Debug, Clone, PartialEq)]
pub enum Values {
    /// Exactly one value per cell, in cell order.
    Explicit(Vec<Raw>),
    /// The storage's (or bitfield's) minimum and maximum: see [`extremes`].
    Extremes,
    /// Edge values that break a wrong codec: see [`specials`].
    Specials,
}

impl Values {
    const fn label(&self) -> &'static str {
        match self {
            Self::Explicit(_) => "explicit",
            Self::Extremes => "extremes",
            Self::Specials => "specials",
        }
    }
}

/// `binary32` special values, as bit patterns: ±0, ±Inf, quiet and
/// signalling NaNs with distinct payloads (and a negative one of each), the
/// smallest and largest subnormals (both signs), the smallest normal, the
/// largest finite (both signs) and 1.0.
pub const F32_SPECIAL_BITS: &[u32] = &[
    0x0000_0000, // +0
    0x8000_0000, // -0
    0x7F80_0000, // +Inf
    0xFF80_0000, // -Inf
    0x7FC0_0000, // quiet NaN, payload 0 (the canonical NaN)
    0x7FC0_1234, // quiet NaN, payload 0x1234
    0xFFC0_0001, // negative quiet NaN, payload 1
    0x7F80_0001, // signalling NaN, payload 1 (smallest)
    0x7FA0_5678, // signalling NaN, payload 0x205678
    0xFFBF_FFFF, // negative signalling NaN, largest payload
    0x0000_0001, // smallest subnormal
    0x8000_0001, // negative smallest subnormal
    0x007F_FFFF, // largest subnormal
    0x807F_FFFF, // negative largest subnormal
    0x0080_0000, // smallest normal
    0x7F7F_FFFF, // largest finite (f32::MAX)
    0xFF7F_FFFF, // lowest finite (f32::MIN)
    0x3F80_0000, // 1.0
];

/// `binary64` special values, as bit patterns (the same list as
/// [`F32_SPECIAL_BITS`]).
pub const F64_SPECIAL_BITS: &[u64] = &[
    0x0000_0000_0000_0000, // +0
    0x8000_0000_0000_0000, // -0
    0x7FF0_0000_0000_0000, // +Inf
    0xFFF0_0000_0000_0000, // -Inf
    0x7FF8_0000_0000_0000, // quiet NaN, payload 0 (the canonical NaN)
    0x7FF8_0000_0000_1234, // quiet NaN, payload 0x1234
    0xFFF8_0000_0000_0001, // negative quiet NaN, payload 1
    0x7FF0_0000_0000_0001, // signalling NaN, payload 1 (smallest)
    0x7FF4_0000_0005_6789, // signalling NaN, payload 0x4_0000_0005_6789
    0xFFF7_FFFF_FFFF_FFFF, // negative signalling NaN, largest payload
    0x0000_0000_0000_0001, // smallest subnormal
    0x8000_0000_0000_0001, // negative smallest subnormal
    0x000F_FFFF_FFFF_FFFF, // largest subnormal
    0x800F_FFFF_FFFF_FFFF, // negative largest subnormal
    0x0010_0000_0000_0000, // smallest normal
    0x7FEF_FFFF_FFFF_FFFF, // largest finite (f64::MAX)
    0xFFEF_FFFF_FFFF_FFFF, // lowest finite (f64::MIN)
    0x3FF0_0000_0000_0000, // 1.0
];

/// Inclusive range of the integer a cell of `encoding` stores: the
/// bitfield's `0..=mask >> shift`, or the storage's range.
fn stored_range(encoding: &Encoding) -> (i128, i128) {
    match encoding.bitfield {
        Some(Bitfield { mask, shift }) => (0, i128::from(mask.checked_shr(shift).unwrap_or(0))),
        None => encoding.storage.int_range(),
    }
}

/// The [`Values::Extremes`] sequence for `encoding`: `[min, max]` of the
/// stored value. Floats: the lowest and largest finite values
/// (`f32::MIN`/`f32::MAX`, `f64::MIN`/`f64::MAX`). A bitfield: `0` and the
/// field's all-ones value. Otherwise [`Storage::int_range`] (so the `u20`
/// maximum follows [`U20_BIT19_SIGN_EXTENDS`]).
#[must_use]
pub fn extremes(encoding: &Encoding) -> Vec<Raw> {
    if encoding.bitfield.is_none() {
        match encoding.storage {
            Storage::F32 => return vec![Raw::F32(f32::MIN), Raw::F32(f32::MAX)],
            Storage::F64 => return vec![Raw::F64(f64::MIN), Raw::F64(f64::MAX)],
            _ => {}
        }
    }
    let (min, max) = stored_range(encoding);
    vec![Raw::Int(min), Raw::Int(max)]
}

/// The [`Values::Specials`] sequence for `encoding`.
///
/// Floats: [`F32_SPECIAL_BITS`] / [`F64_SPECIAL_BITS`]. Integers (and
/// bitfields, keeping only values made of the field's bits), in this order
/// and without repeats: `0`, `1`, `max`,
/// `max - 1`, `min`, `min + 1`, `-1` (signed storage), the top bit alone
/// (unsigned storage and bitfields: `0x80..`), and a byte-order pattern
/// (the top `bits` bits of `0x0102030405060708`, so every byte differs).
#[must_use]
pub fn specials(encoding: &Encoding) -> Vec<Raw> {
    if encoding.bitfield.is_none() {
        match encoding.storage {
            Storage::F32 => {
                return F32_SPECIAL_BITS
                    .iter()
                    .map(|bits| Raw::F32(f32::from_bits(*bits)))
                    .collect();
            }
            Storage::F64 => {
                return F64_SPECIAL_BITS
                    .iter()
                    .map(|bits| Raw::F64(f64::from_bits(*bits)))
                    .collect();
            }
            _ => {}
        }
    }
    let (min, max) = stored_range(encoding);
    // Bits of the unsigned value: the field's width, or the storage's.
    let bits = match encoding.bitfield {
        Some(_) => 128 - max.leading_zeros(),
        None => encoding.storage.value_bits(),
    };
    let signed = encoding.bitfield.is_none() && encoding.storage.is_signed();
    let mut candidates = vec![0, 1, max, max - 1, min, min + 1];
    if signed {
        candidates.push(-1);
    } else if bits > 0 {
        candidates.push(1_i128 << (bits - 1));
    }
    if (1..=64).contains(&bits) {
        candidates.push(i128::from(0x0102_0304_0506_0708_u64 >> (64 - bits)));
    }
    let mut out: Vec<i128> = Vec::with_capacity(candidates.len());
    // A bitfield value must use only the field's bits (`max` is the field's
    // all-ones value), which matters for a non-contiguous mask.
    let fits = |value: i128| {
        (min..=max).contains(&value) && (encoding.bitfield.is_none() || value & !max == 0)
    };
    for value in candidates {
        if fits(value) && !out.contains(&value) {
            out.push(value);
        }
    }
    out.into_iter().map(Raw::Int).collect()
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Why a plan could not be generated.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SyntheticError {
    /// The ROM is smaller than the synthetic marker.
    #[error("ROM size {size} is smaller than the 32-byte synthetic marker")]
    RomTooSmall {
        /// Requested size.
        size: usize,
    },
    /// The ROM is larger than [`MAX_ROM_SIZE`].
    #[error("ROM size {size} is larger than the {max}-byte limit")]
    RomTooLarge {
        /// Requested size.
        size: usize,
        /// [`MAX_ROM_SIZE`].
        max: usize,
    },
    /// A plan or table name is empty, too long, or has a character outside
    /// ASCII letters, digits, `-`, `_` and `.`.
    #[error("invalid name {name:?}: use 1..=64 ASCII letters, digits, '-', '_' or '.'")]
    InvalidName {
        /// The name.
        name: String,
    },
    /// Two tables have the same name.
    #[error("duplicate table name {name:?}")]
    DuplicateTable {
        /// The name.
        name: String,
    },
    /// A table has no cells (or `cols * rows` overflows).
    #[error("table {table:?}: dims {cols}x{rows} give no usable cell count")]
    BadDims {
        /// Table name.
        table: String,
        /// Columns.
        cols: usize,
        /// Rows.
        rows: usize,
    },
    /// A table does not fit inside the ROM.
    #[error("table {table:?}: {length} byte(s) at {address:#x} do not fit in the {size}-byte ROM")]
    OutOfBounds {
        /// Table name.
        table: String,
        /// Table address.
        address: usize,
        /// Table length in bytes (saturated if it overflowed).
        length: usize,
        /// ROM size.
        size: usize,
    },
    /// A table starts inside the synthetic marker.
    #[error("table {table:?}: address {address:#x} is inside the 32-byte synthetic marker")]
    OverlapsMarker {
        /// Table name.
        table: String,
        /// Table address.
        address: usize,
    },
    /// An explicit value list does not have one value per cell.
    #[error("table {table:?}: {got} explicit value(s) for {cells} cell(s)")]
    ValueCount {
        /// Table name.
        table: String,
        /// Cells in the table.
        cells: usize,
        /// Values given.
        got: usize,
    },
    /// A generator's sequence is longer than the table.
    #[error(
        "table {table:?}: the {generator} sequence has {needed} value(s) but the table has only {cells} cell(s)"
    )]
    TooFewCells {
        /// Table name.
        table: String,
        /// Generator name.
        generator: &'static str,
        /// Length of the generator's sequence.
        needed: usize,
        /// Cells in the table.
        cells: usize,
    },
    /// The plan asks for something with no defined meaning.
    #[error("table {table:?}: {reason}")]
    Unsupported {
        /// Table name.
        table: String,
        /// What is wrong.
        reason: &'static str,
    },
    /// The reference codec refused a table.
    #[error("table {table:?}: {source}")]
    Codec {
        /// Table name.
        table: String,
        /// The codec's error.
        source: ReferenceError,
    },
}

// ---------------------------------------------------------------------------
// Manifest
// ---------------------------------------------------------------------------

/// The expected values of a generated ROM.
#[derive(Debug, Clone)]
pub struct Manifest {
    /// The plan's name.
    pub plan: String,
    /// ROM size in bytes.
    pub rom_size: usize,
    /// The fill byte.
    pub fill: u8,
    /// FNV-1a 64 of the ROM bytes (binds the manifest to its ROM).
    pub rom_fnv1a64: u64,
    /// Tables, in plan (write) order.
    pub tables: Vec<TableManifest>,
}

/// One table of a [`Manifest`].
#[derive(Debug, Clone)]
pub struct TableManifest {
    /// Table name.
    pub name: String,
    /// Position in the plan (0-based): the order tables were written.
    pub write_order: usize,
    /// ROM address of the first cell.
    pub address: usize,
    /// How the cells are stored.
    pub encoding: Encoding,
    /// Cell grid.
    pub dims: Dims,
    /// Later tables whose bytes overlap this one's, in write order.
    pub overlapped_by: Vec<String>,
    /// The plan's note, if any.
    pub note: Option<String>,
    /// Every cell, in cell order.
    pub cells: Vec<CellManifest>,
}

impl TableManifest {
    /// Bytes the table occupies.
    #[must_use]
    pub const fn byte_len(&self) -> usize {
        self.cells.len() * self.encoding.storage.cell_bytes()
    }

    /// One past the table's last byte.
    #[must_use]
    pub const fn end(&self) -> usize {
        self.address + self.byte_len()
    }
}

/// One cell of a [`TableManifest`].
#[derive(Debug, Clone, Copy)]
pub struct CellManifest {
    /// Cell index (row-major).
    pub index: usize,
    /// Absolute ROM address of the cell.
    pub offset: usize,
    /// The value the cell decodes to in the final ROM.
    pub expected: Raw,
    /// The value the plan wrote.
    pub planned: Raw,
}

impl CellManifest {
    /// Whether a later table changed this cell's value (bit-for-bit).
    #[must_use]
    pub fn clobbered(&self) -> bool {
        !same_bits(self.expected, self.planned)
    }
}

/// Whether two raw values are identical: integers equal, floats of the same
/// width with the same bit pattern (so NaN equals the same NaN, and `-0.0`
/// differs from `+0.0`).
#[must_use]
pub fn same_bits(a: Raw, b: Raw) -> bool {
    match (a, b) {
        (Raw::Int(a), Raw::Int(b)) => a == b,
        (Raw::F32(a), Raw::F32(b)) => a.to_bits() == b.to_bits(),
        (Raw::F64(a), Raw::F64(b)) => a.to_bits() == b.to_bits(),
        _ => false,
    }
}

/// A raw value as the manifest writes it: a decimal integer, or a fixed-width
/// `0x`-prefixed lowercase hex bit pattern for floats.
#[must_use]
pub fn raw_text(raw: Raw) -> String {
    match raw {
        Raw::Int(value) => value.to_string(),
        Raw::F32(value) => format!("0x{:08x}", value.to_bits()),
        Raw::F64(value) => format!("0x{:016x}", value.to_bits()),
    }
}

/// IEEE 754 class of a float from its sign, exponent and fraction fields
/// (integer arithmetic only): `+zero`, `-subnormal`, `+normal`, `-inf`,
/// `+qnan`, `+snan`, ...
fn float_class(
    sign: bool,
    exponent_all_ones: bool,
    exponent_zero: bool,
    fraction: u64,
    quiet_bit: u64,
) -> String {
    let class = if exponent_zero {
        if fraction == 0 { "zero" } else { "subnormal" }
    } else if exponent_all_ones {
        if fraction == 0 {
            "inf"
        } else if fraction & quiet_bit != 0 {
            "qnan"
        } else {
            "snan"
        }
    } else {
        "normal"
    };
    format!("{}{class}", if sign { '-' } else { '+' })
}

/// The manifest `class` of a float raw value; `None` for integers.
#[must_use]
pub fn raw_class(raw: Raw) -> Option<String> {
    match raw {
        Raw::Int(_) => None,
        Raw::F32(value) => {
            let bits = value.to_bits();
            let exponent = (bits >> 23) & 0xFF;
            Some(float_class(
                bits >> 31 == 1,
                exponent == 0xFF,
                exponent == 0,
                u64::from(bits & 0x007F_FFFF),
                0x0040_0000,
            ))
        }
        Raw::F64(value) => {
            let bits = value.to_bits();
            let exponent = (bits >> 52) & 0x7FF;
            Some(float_class(
                bits >> 63 == 1,
                exponent == 0x7FF,
                exponent == 0,
                bits & 0x000F_FFFF_FFFF_FFFF,
                0x0008_0000_0000_0000,
            ))
        }
    }
}

/// FNV-1a 64 of `bytes`.
#[must_use]
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash = 0xCBF2_9CE4_8422_2325_u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01B3);
    }
    hash
}

/// A TOML basic string (quotes, backslashes and control characters escaped).
fn toml_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            c if c.is_control() => {
                let _ = write!(out, "\\u{:04x}", u32::from(c));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

const fn endian_name(endian: Endian) -> &'static str {
    match endian {
        Endian::Big => "big",
        Endian::Little => "little",
    }
}

const fn layout_name(layout: Layout) -> &'static str {
    match layout {
        Layout::Default => "default",
        Layout::BoschSubtract => "bosch-subtract",
    }
}

impl Manifest {
    /// The table named `name`.
    #[must_use]
    pub fn table(&self, name: &str) -> Option<&TableManifest> {
        self.tables.iter().find(|table| table.name == name)
    }

    /// The manifest as deterministic TOML text (format in the module docs).
    #[must_use]
    pub fn to_toml(&self) -> String {
        let mut out = String::new();
        // Writing to a String cannot fail; the results are ignored.
        let _ = self.write_toml(&mut out);
        out
    }

    fn write_toml(&self, out: &mut String) -> fmt::Result {
        writeln!(
            out,
            "# Synthetic ROM manifest: the expected raw value of every cell of every table."
        )?;
        writeln!(
            out,
            "# Generated by rr_testkit::reference::synthetic (US-RR-10-6); do not edit."
        )?;
        writeln!(
            out,
            "# The ROM is SYNTHETIC test data (marker at offset 0), not a real ECU image."
        )?;
        writeln!(out, "schema = {MANIFEST_SCHEMA}")?;
        writeln!(out, "synthetic = true")?;
        writeln!(out, "plan = {}", toml_string(&self.plan))?;
        writeln!(out, "rom_size = {}", self.rom_size)?;
        writeln!(out, "fill = {:#04x}", self.fill)?;
        writeln!(out, "rom_fnv1a64 = \"0x{:016x}\"", self.rom_fnv1a64)?;
        writeln!(out, "u20_bit19_sign_extends = {U20_BIT19_SIGN_EXTENDS}")?;
        let marker_hex: Vec<String> = MARKER.iter().map(|b| format!("{b:02x}")).collect();
        writeln!(
            out,
            "marker = {{ offset = 0x0, length = {MARKER_LEN}, hex = \"{}\" }}",
            marker_hex.join(" ")
        )?;
        writeln!(out, "overlap_rule = {}", toml_string(OVERLAP_RULE))?;
        writeln!(
            out,
            "raw_format = \"integers (and bitfields, bosch-subtract values): decimal string; f32/f64: IEEE 754 bit pattern as fixed-width 0x hex\""
        )?;
        for table in &self.tables {
            writeln!(out)?;
            writeln!(out, "[[table]]")?;
            writeln!(out, "name = {}", toml_string(&table.name))?;
            writeln!(out, "write_order = {}", table.write_order)?;
            writeln!(out, "address = {:#x}", table.address)?;
            writeln!(out, "end = {:#x}", table.end())?;
            writeln!(out, "storage = \"{}\"", table.encoding.storage.name())?;
            writeln!(out, "endian = \"{}\"", endian_name(table.encoding.endian))?;
            writeln!(out, "layout = \"{}\"", layout_name(table.encoding.layout))?;
            if let Some(Bitfield { mask, shift }) = table.encoding.bitfield {
                writeln!(
                    out,
                    "bitfield = {{ mask = \"{mask:#x}\", shift = {shift} }}"
                )?;
            }
            writeln!(out, "cols = {}", table.dims.cols)?;
            writeln!(out, "rows = {}", table.dims.rows)?;
            let overlapped: Vec<String> =
                table.overlapped_by.iter().map(|n| toml_string(n)).collect();
            writeln!(out, "overlapped_by = [{}]", overlapped.join(", "))?;
            if let Some(note) = &table.note {
                writeln!(out, "note = {}", toml_string(note))?;
            }
            writeln!(out, "cells = [")?;
            for cell in &table.cells {
                write!(
                    out,
                    "  {{ index = {}, offset = {:#x}, raw = \"{}\"",
                    cell.index,
                    cell.offset,
                    raw_text(cell.expected)
                )?;
                if let Some(class) = raw_class(cell.expected) {
                    write!(out, ", class = \"{class}\"")?;
                }
                if cell.clobbered() {
                    write!(
                        out,
                        ", clobbered = true, planned = \"{}\"",
                        raw_text(cell.planned)
                    )?;
                }
                writeln!(out, " }},")?;
            }
            writeln!(out, "]")?;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Generation
// ---------------------------------------------------------------------------

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_NAME_LEN
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}

/// A table after its plan has been checked.
struct Placed {
    address: usize,
    end: usize,
    planned: Vec<Raw>,
}

/// Bosch-subtract values from stored cells: `value[i] = 2^(8W) - (s[i] +
/// ... + s[N-1])`.
fn bosch_values(stored: &[Raw], storage: Storage, table: &str) -> Result<Vec<Raw>, SyntheticError> {
    let unsupported = |reason| SyntheticError::Unsupported {
        table: table.to_owned(),
        reason,
    };
    if storage.is_float() || storage == Storage::U20 {
        return Err(unsupported(
            "bosch-subtract is defined for 1/2/3/4/8-byte integer storage only",
        ));
    }
    let mut running = 1_i128 << storage.value_bits();
    let mut values = vec![Raw::Int(0); stored.len()];
    for (slot, cell) in values.iter_mut().zip(stored).rev() {
        let Raw::Int(cell) = cell else {
            return Err(unsupported("bosch-subtract stored cell is not an integer"));
        };
        running = running
            .checked_sub(*cell)
            .ok_or_else(|| unsupported("bosch-subtract sum overflows"))?;
        *slot = Raw::Int(running);
    }
    Ok(values)
}

/// Checks one table and resolves its values (one per cell).
fn place(table: &TablePlan, size: usize) -> Result<Placed, SyntheticError> {
    let name = || table.name.clone();
    let cells = table
        .dims
        .cells()
        .filter(|cells| *cells > 0)
        .ok_or_else(|| SyntheticError::BadDims {
            table: name(),
            cols: table.dims.cols,
            rows: table.dims.rows,
        })?;
    let length = cells.saturating_mul(table.encoding.storage.cell_bytes());
    let end = table
        .address
        .checked_add(length)
        .filter(|end| *end <= size)
        .ok_or_else(|| SyntheticError::OutOfBounds {
            table: name(),
            address: table.address,
            length,
            size,
        })?;
    if table.address < MARKER_LEN {
        return Err(SyntheticError::OverlapsMarker {
            table: name(),
            address: table.address,
        });
    }
    let planned = match &table.values {
        Values::Explicit(values) => {
            if values.len() != cells {
                return Err(SyntheticError::ValueCount {
                    table: name(),
                    cells,
                    got: values.len(),
                });
            }
            values.clone()
        }
        generator @ (Values::Extremes | Values::Specials) => {
            let sequence = match generator {
                Values::Extremes => extremes(&table.encoding),
                _ => specials(&table.encoding),
            };
            if sequence.is_empty() || sequence.len() > cells {
                return Err(SyntheticError::TooFewCells {
                    table: name(),
                    generator: generator.label(),
                    needed: sequence.len(),
                    cells,
                });
            }
            let stored: Vec<Raw> = sequence.iter().copied().cycle().take(cells).collect();
            match table.encoding.layout {
                Layout::Default => stored,
                Layout::BoschSubtract => {
                    bosch_values(&stored, table.encoding.storage, &table.name)?
                }
            }
        }
    };
    Ok(Placed {
        address: table.address,
        end,
        planned,
    })
}

/// Generates the ROM and its manifest from `plan` (see the module docs for
/// the rules).
///
/// # Errors
///
/// A [`SyntheticError`] for the first problem found, in plan order: a ROM
/// size outside [`MARKER_LEN`]..=[`MAX_ROM_SIZE`], an invalid or duplicate
/// name, a table with no cells, out of bounds or inside the marker, a wrong
/// number of explicit values, a generator longer than its table, or a value
/// the reference codec cannot store.
pub fn generate(plan: &Plan) -> Result<(Vec<u8>, Manifest), SyntheticError> {
    if plan.size < MARKER_LEN {
        return Err(SyntheticError::RomTooSmall { size: plan.size });
    }
    if plan.size > MAX_ROM_SIZE {
        return Err(SyntheticError::RomTooLarge {
            size: plan.size,
            max: MAX_ROM_SIZE,
        });
    }
    if !valid_name(&plan.name) {
        return Err(SyntheticError::InvalidName {
            name: plan.name.clone(),
        });
    }
    let mut rom = vec![plan.fill; plan.size];
    if let Some(head) = rom.get_mut(..MARKER_LEN) {
        head.copy_from_slice(MARKER);
    }

    let mut placed: Vec<Placed> = Vec::with_capacity(plan.tables.len());
    for (index, table) in plan.tables.iter().enumerate() {
        if !valid_name(&table.name) {
            return Err(SyntheticError::InvalidName {
                name: table.name.clone(),
            });
        }
        if plan.tables.iter().take(index).any(|t| t.name == table.name) {
            return Err(SyntheticError::DuplicateTable {
                name: table.name.clone(),
            });
        }
        let one = place(table, plan.size)?;
        encode_table(&one.planned, &table.encoding, &mut rom, one.address).map_err(|source| {
            SyntheticError::Codec {
                table: table.name.clone(),
                source,
            }
        })?;
        placed.push(one);
    }

    let mut tables = Vec::with_capacity(plan.tables.len());
    for (index, (table, one)) in plan.tables.iter().zip(&placed).enumerate() {
        let count = one.planned.len();
        let decoded =
            decode_table(&rom, one.address, count, &table.encoding).map_err(|source| {
                SyntheticError::Codec {
                    table: table.name.clone(),
                    source,
                }
            })?;
        let stride = table.encoding.storage.cell_bytes();
        let cells = decoded
            .iter()
            .zip(&one.planned)
            .enumerate()
            .map(|(cell, (expected, planned))| CellManifest {
                index: cell,
                offset: one.address + cell * stride,
                expected: *expected,
                planned: *planned,
            })
            .collect();
        let overlapped_by = plan
            .tables
            .iter()
            .zip(&placed)
            .skip(index + 1)
            .filter(|(_, later)| later.address < one.end && one.address < later.end)
            .map(|(later, _)| later.name.clone())
            .collect();
        tables.push(TableManifest {
            name: table.name.clone(),
            write_order: index,
            address: one.address,
            encoding: table.encoding,
            dims: table.dims,
            overlapped_by,
            note: table.note.clone(),
            cells,
        });
    }
    let manifest = Manifest {
        plan: plan.name.clone(),
        rom_size: plan.size,
        fill: plan.fill,
        rom_fnv1a64: fnv1a64(&rom),
        tables,
    };
    Ok((rom, manifest))
}

// ---------------------------------------------------------------------------
// The standard plan
// ---------------------------------------------------------------------------

/// Note on every non-bitfield `u20` table of the standard plan.
pub const U20_NOTE: &str = "u20 raw values sign-extend imm20 bit 19 (US-RR-19; U20_BIT19_SIGN_EXTENDS = true), so the range is -524288..=524287; the low nibble of each cell's first byte is opcode and keeps the fill byte's";

/// Lays tables out one after another.
struct Cursor {
    at: usize,
    tables: Vec<TablePlan>,
}

impl Cursor {
    const ALIGN: usize = 0x10;

    /// Cells a generator's sequence needs (an explicit list: its length).
    fn natural_cells(encoding: &Encoding, values: &Values) -> usize {
        match values {
            Values::Explicit(values) => values.len(),
            Values::Extremes => extremes(encoding).len(),
            Values::Specials => specials(encoding).len(),
        }
    }

    /// Places a one-row table sized to its values at the cursor.
    fn row(&mut self, name: String, encoding: Encoding, values: Values) -> usize {
        let cells = Self::natural_cells(&encoding, &values);
        self.push(TablePlan::new(name, 0, encoding, Dims::row(cells), values))
    }

    /// Places `table` at the cursor; returns its address.
    fn push(&mut self, mut table: TablePlan) -> usize {
        let address = self.at;
        table.address = address;
        let bytes = table.dims.cells().unwrap_or(0) * table.encoding.storage.cell_bytes();
        self.at = (address + bytes).next_multiple_of(Self::ALIGN);
        self.tables.push(table);
        address
    }

    /// Places `table` at `address` without moving the cursor (overlaps).
    fn at(&mut self, address: usize, mut table: TablePlan) {
        table.address = address;
        self.tables.push(table);
    }

    /// Skips `bytes` (reserved for tables placed with [`Cursor::at`]).
    fn reserve(&mut self, bytes: usize) -> usize {
        let address = self.at;
        self.at = (address + bytes).next_multiple_of(Self::ALIGN);
        address
    }
}

fn ints(values: &[i128]) -> Values {
    Values::Explicit(values.iter().copied().map(Raw::Int).collect())
}

/// The standard plan (`"standard"`), used by public CI.
///
/// In write order:
///
/// 1. For every storage type and both byte orders (`u20`: big-endian only),
///    an `<storage>-<be|le>-extremes` table ([`extremes`]: min and max) and an
///    `<storage>-<be|le>-specials` table ([`specials`]: NaN quiet and
///    signalling with distinct payloads, ±Inf, ±0, min/max subnormal, ... for
///    floats; 0, ±1, top bit, byte-order pattern ... for integers).
/// 2. Bitfield tables on `u8`, `i16` (a signed storage: the field is still
///    unsigned), `u20`, `u32` (a non-contiguous mask) and `u64` (the full
///    mask, whose value exceeds `i64::MAX`), an extremes bitfield on each of
///    `i8`, `u24`, `i24` (the full mask), `i32` and `i64` (so every integer
///    storage has a field holding its 0 and all-ones), and three tables
///    sharing the same two `u16` cells with disjoint masks
///    (`bits-shared-hi/mid/lo`), then a fourth (`bits-shared-clash`) whose
///    mask lies inside `mid`'s, so it clobbers `mid` and nothing else.
/// 3. Bosch-subtract tables (`u8` explicit, `u16` / `u64` extremes, `i32`
///    specials) and a `3x2` grid.
/// 4. Overlapping tables: `overlap-a-u16-be` overwritten in part by
///    `overlap-b-u8` and `overlap-c-i32-le` (unaligned, running past `a`'s
///    end), `overlap-d-f32-be` (1.0) whose last byte `overlap-e-u8` changes,
///    and `overlap-bosch-u8` whose last stored cell `overlap-f-u8` changes,
///    which changes every value of the Bosch table.
/// 5. `end-of-rom-f64-be-specials`, ending exactly at the end of the ROM
///    (the ROM size is the next multiple of 256 after the tables).
#[must_use]
pub fn standard_plan() -> Plan {
    let mut cursor = Cursor {
        at: 0x40,
        tables: Vec::new(),
    };

    // 1. Extremes and specials of every storage, both byte orders.
    for storage in Storage::ALL {
        for (endian, tag) in [(Endian::Big, "be"), (Endian::Little, "le")] {
            if storage == Storage::U20 && endian == Endian::Little {
                continue;
            }
            let encoding = Encoding::new(storage, endian);
            for values in [Values::Extremes, Values::Specials] {
                let name = format!("{}-{tag}-{}", storage.name(), values.label());
                let cells = Cursor::natural_cells(&encoding, &values);
                let mut table = TablePlan::new(name, 0, encoding, Dims::row(cells), values);
                if storage == Storage::U20 {
                    table = table.with_note(U20_NOTE);
                }
                cursor.push(table);
            }
        }
    }

    // 2. Bitfields.
    let bitfields: [(Storage, Endian, u64, &str, Values); 10] = [
        (
            Storage::U8,
            Endian::Big,
            0x3C,
            "bitfield-u8-be-mask-0x3c",
            Values::Extremes,
        ),
        (
            Storage::I16,
            Endian::Little,
            0xFF00,
            "bitfield-i16-le-mask-0xff00",
            Values::Specials,
        ),
        (
            Storage::U20,
            Endian::Big,
            0xF_0000,
            "bitfield-u20-be-mask-0xf0000",
            Values::Specials,
        ),
        (
            Storage::U32,
            Endian::Big,
            0x8000_0001,
            "bitfield-u32-be-mask-0x80000001",
            Values::Specials,
        ),
        (
            Storage::U64,
            Endian::Little,
            u64::MAX,
            "bitfield-u64-le-mask-all",
            Values::Extremes,
        ),
        // One bitfield on every other integer storage, so each width and
        // signedness has a field holding its 0 and all-ones values.
        (
            Storage::I8,
            Endian::Little,
            0xF0,
            "bitfield-i8-le-mask-0xf0",
            Values::Extremes,
        ),
        (
            Storage::U24,
            Endian::Little,
            0x0F_FF00,
            "bitfield-u24-le-mask-0xfff00",
            Values::Extremes,
        ),
        (
            Storage::I24,
            Endian::Big,
            0xFF_FFFF,
            "bitfield-i24-be-mask-all",
            Values::Extremes,
        ),
        (
            Storage::I32,
            Endian::Little,
            0x7FFF_FFFE,
            "bitfield-i32-le-mask-0x7ffffffe",
            Values::Extremes,
        ),
        (
            Storage::I64,
            Endian::Big,
            0xFFFF_0000_0000_0000,
            "bitfield-i64-be-mask-0xffff000000000000",
            Values::Extremes,
        ),
    ];
    for (storage, endian, mask, name, values) in bitfields {
        let encoding = Encoding::new(storage, endian).with_bitfield(Bitfield::new(mask, None));
        cursor.row(name.to_owned(), encoding, values);
    }
    let shared = cursor.reserve(2 * 2);
    for (name, mask, values) in [
        ("bits-shared-hi", 0xF000, Values::Extremes),
        ("bits-shared-mid", 0x0FF0, Values::Extremes),
        ("bits-shared-lo", 0x000F, Values::Extremes),
        ("bits-shared-clash", 0x00F0, ints(&[0xA, 0x5])),
    ] {
        let encoding =
            Encoding::new(Storage::U16, Endian::Big).with_bitfield(Bitfield::new(mask, None));
        cursor.at(
            shared,
            TablePlan::new(name, 0, encoding, Dims::row(2), values),
        );
    }

    // 3. Bosch subtract and a grid.
    let bosch = |storage, endian| Encoding::new(storage, endian).with_layout(Layout::BoschSubtract);
    cursor.row(
        "bosch-u8-be-explicit".to_owned(),
        bosch(Storage::U8, Endian::Big),
        ints(&[10, 20, 156]),
    );
    cursor.push(TablePlan::new(
        "bosch-u16-be-extremes",
        0,
        bosch(Storage::U16, Endian::Big),
        Dims::row(4),
        Values::Extremes,
    ));
    cursor.row(
        "bosch-i32-le-specials".to_owned(),
        bosch(Storage::I32, Endian::Little),
        Values::Specials,
    );
    cursor.push(TablePlan::new(
        "bosch-u64-be-extremes",
        0,
        bosch(Storage::U64, Endian::Big),
        Dims::row(2),
        Values::Extremes,
    ));
    cursor.push(TablePlan::new(
        "grid-u16-le-3x2",
        0,
        Encoding::new(Storage::U16, Endian::Little),
        Dims::grid(3, 2),
        ints(&[0x0001, 0x0203, 0x8000, 0xFFFF, 0x1234, 0xFEDC]),
    ));

    // 4. Overlaps (last write wins).
    let u8_be = Encoding::new(Storage::U8, Endian::Big);
    let a = cursor.reserve(10);
    cursor.at(
        a,
        TablePlan::new(
            "overlap-a-u16-be",
            0,
            Encoding::new(Storage::U16, Endian::Big),
            Dims::row(4),
            ints(&[0x1111, 0x2222, 0x3333, 0x4444]),
        ),
    );
    cursor.at(
        a + 3,
        TablePlan::new("overlap-b-u8", 0, u8_be, Dims::row(2), ints(&[0xAB, 0xCD])),
    );
    cursor.at(
        a + 6,
        TablePlan::new(
            "overlap-c-i32-le",
            0,
            Encoding::new(Storage::I32, Endian::Little),
            Dims::scalar(),
            ints(&[-2]),
        ),
    );
    let d = cursor.reserve(4);
    cursor.at(
        d,
        TablePlan::new(
            "overlap-d-f32-be",
            0,
            Encoding::new(Storage::F32, Endian::Big),
            Dims::scalar(),
            Values::Explicit(vec![Raw::F32(1.0)]),
        ),
    );
    cursor.at(
        d + 3,
        TablePlan::new("overlap-e-u8", 0, u8_be, Dims::scalar(), ints(&[0x01])),
    );
    let f = cursor.reserve(3);
    cursor.at(
        f,
        TablePlan::new(
            "overlap-bosch-u8",
            0,
            bosch(Storage::U8, Endian::Big),
            Dims::row(3),
            ints(&[10, 20, 156]),
        ),
    );
    cursor.at(
        f + 2,
        TablePlan::new("overlap-f-u8", 0, u8_be, Dims::scalar(), ints(&[20])),
    );

    // 5. A table ending exactly at the end of the ROM.
    let end_encoding = Encoding::new(Storage::F64, Endian::Big);
    let end_cells = specials(&end_encoding).len();
    let end_bytes = end_cells * Storage::F64.cell_bytes();
    let size = (cursor.at + end_bytes).next_multiple_of(0x100);
    cursor.at(
        size - end_bytes,
        TablePlan::new(
            "end-of-rom-f64-be-specials",
            0,
            end_encoding,
            Dims::row(end_cells),
            Values::Specials,
        ),
    );

    Plan {
        name: "standard".to_owned(),
        size,
        fill: 0xFF,
        tables: cursor.tables,
    }
}

// ---------------------------------------------------------------------------
// Committed output (US-RR-10-7)
// ---------------------------------------------------------------------------

/// The plans whose output is committed under `fixtures/public/synthetic/`
/// and regenerated by `cargo xtask gen-synthetic` (which fails with
/// `--check` on any byte difference). Plan names are unique, so each plan's
/// [`output_files`] have distinct names.
#[must_use]
pub fn published_plans() -> Vec<Plan> {
    vec![standard_plan()]
}

/// Suffix of a plan's ROM image file.
pub const ROM_SUFFIX: &str = ".bin";

/// Suffix of a plan's manifest file.
pub const MANIFEST_SUFFIX: &str = ".manifest.toml";

/// The files `plan` generates, as `(file name, bytes)`: the ROM image
/// `<name>.bin` and its manifest `<name>.manifest.toml` ([`Manifest::to_toml`],
/// `\n` line endings only). The bytes depend only on the plan (see
/// "Determinism" in the module docs).
///
/// # Errors
///
/// Whatever [`generate`] rejects in the plan.
pub fn output_files(plan: &Plan) -> Result<Vec<(String, Vec<u8>)>, SyntheticError> {
    let (rom, manifest) = generate(plan)?;
    Ok(vec![
        (format!("{}{ROM_SUFFIX}", plan.name), rom),
        (
            format!("{}{MANIFEST_SUFFIX}", plan.name),
            manifest.to_toml().into_bytes(),
        ),
    ])
}
