//! Naive reference codec (US-RR-10, PROJECT.md T2(b), D-024).
//!
//! A deliberately simple second implementation of byte encoding and decoding,
//! used to cross-check rr-core's Encoding. It shares **no code** with rr-core:
//! nothing in this module (or anything it reaches) names a workspace crate,
//! which `cargo xtask check-reference-isolation` enforces. It does not use the
//! [`kat`](crate::kat) fixture types either; it has its own small vocabulary
//! ([`Storage`], [`Endian`], [`Layout`], [`Bitfield`], [`Encoding`], [`Raw`])
//! and tests convert between the two at the test boundary.
//!
//! "Naive" is the point: every multi-byte value is assembled and split one
//! byte at a time with explicit shifts (`acc << 8 | byte`, `value >> 8 * k`).
//! No `from_be_bytes` / `to_le_bytes` / byte-order crates. Floats are built
//! with [`f32::from_bits`] / [`f64::from_bits`] from a hand-assembled integer.
//!
//! # Semantics
//!
//! * **Integers.** A cell of `W` bytes is read into a `8W`-bit pattern
//!   (big-endian: first byte most significant; little-endian: first byte least
//!   significant). Unsigned storage is the pattern; signed storage subtracts
//!   `2^(8W)` when the top bit is set (two's complement).
//! * **Bitfield** (`mask`, `shift`): `raw = (pattern & mask) >> shift`, on the
//!   decoded pattern (not the byte image), unsigned whatever the storage's
//!   signedness. `shift` defaults to the mask's trailing zeros. Encoding is
//!   read-modify-write: bits outside `mask` keep their stored values. A mask
//!   with bits outside the stored width, or a bitfield on float storage, is an
//!   error.
//! * **`u20` (SH-2A `MOVI20`).** The cell is the 3 bytes `c0 c1 c2` starting at
//!   the instruction's second byte; `imm20 = (c0 >> 4) * 65536 + c1 * 256 + c2`
//!   (legacy `RomAttributeParser.java:137-158,181-191`). The low nibble of `c0`
//!   is part of the opcode and is preserved on encode. `u20` is big-endian only
//!   (it is an SH-2A instruction field); little-endian is an error. Whether
//!   bit 19 sign-extends is **undecided** (US-RR-19); the single decision point
//!   is [`U20_BIT19_SIGN_EXTENDS`], currently `false` (unsigned, as the name
//!   says).
//! * **Floats.** `f32` / `f64` are IEEE 754 binary32 / binary64 bit patterns;
//!   decoding and encoding are bit-exact (NaN payloads, `-0.0` and subnormals
//!   included).
//! * **Bosch subtract layout** ([`Layout::BoschSubtract`]): a whole-table
//!   layout, so it goes through [`decode_table`] / [`encode_table`]; the
//!   single-cell [`decode`] / [`encode`] reject it. As legacy defines it
//!   (`legacy/src/main/java/com/romraider/maps/DataCell.java:193-207` decode,
//!   `:219-231` and `:312-313` encode; layout attribute `bosch_subtract`,
//!   `maps/Table.java:82,520-526`): for a table of `N` stored cells `s[0..N]`
//!   of `W` bytes each,
//!
//!   ```text
//!   value[i] = 2^(8W) - (s[i] + s[i+1] + ... + s[N-1])
//!   ```
//!
//!   where each `s[j]` is the ordinary cell decode (signedness and bitfield
//!   included, as legacy's per-cell `getValueFromMemory(j)`). The inverse is
//!   `s[N-1] = 2^(8W) - value[N-1]` and `s[i] = value[i+1] - value[i]`. Values
//!   are therefore not bounded by the storage width (`value[N-1]` can be
//!   `2^(8W)`), but every derived `s[i]` must fit its cell: legacy wraps an
//!   out-of-range `s[i]` silently (`(int)` cast), this codec returns an error.
//!   Legacy's `2^(8 * storageType)` is meaningless for float and MOVI20
//!   storage (their `storageType` codes are 99 / 20 / 28), so the layout is
//!   only defined for the 1/2/3/4/8-byte integer storages; others are an error.
//!
//! Every function takes untrusted bytes and offsets: a cell that does not fit
//! the input is [`ReferenceError::ShortInput`], never a panic, and a value that
//! does not fit the storage or bitfield is an error, never silently truncated.
//!
//! # Synthetic ROMs
//!
//! [`synthetic`] builds synthetic ROMs and their expected-values manifests
//! from a value plan, writing with this codec. It lives inside this module so
//! the isolation rule covers it too: its output is a cross-check for rr-core,
//! so it may not use rr-core either.

use std::fmt;

pub mod synthetic;

/// THE `u20` bit-19 decision point (open question, US-RR-19; fixtures/README.md
/// "Open questions").
///
/// `false`: a `u20` raw value is the 20-bit immediate read unsigned
/// (`0..=0xFFFFF`), as the name says. `true` would sign-extend bit 19 (as the
/// SH-2A CPU and legacy's `getMovi20`, `RomAttributeParser.java:181-191`, do),
/// giving `-0x80000..=0x7FFFF`. Decode, encode and the encode range all follow
/// this one constant.
pub const U20_BIT19_SIGN_EXTENDS: bool = false;

/// Storage types the reference codec understands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Storage {
    /// Unsigned 8-bit.
    U8,
    /// Unsigned 16-bit.
    U16,
    /// 20-bit SH-2A `MOVI20` immediate in a 3-byte cell.
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
    /// Every storage type.
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

    /// Lowercase name (`"u8"`, `"i24"`, `"f64"`, ...).
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

    /// Bytes one cell occupies in the input (`u20`: 3).
    #[must_use]
    pub const fn cell_bytes(self) -> usize {
        match self {
            Self::U8 | Self::I8 => 1,
            Self::U16 | Self::I16 => 2,
            Self::U20 | Self::U24 | Self::I24 => 3,
            Self::U32 | Self::I32 | Self::F32 => 4,
            Self::U64 | Self::I64 | Self::F64 => 8,
        }
    }

    /// Bits in the stored value pattern (`u20`: 20, else `8 * cell_bytes`).
    #[must_use]
    pub const fn value_bits(self) -> u32 {
        match self {
            Self::U8 | Self::I8 => 8,
            Self::U16 | Self::I16 => 16,
            Self::U20 => 20,
            Self::U24 | Self::I24 => 24,
            Self::U32 | Self::I32 | Self::F32 => 32,
            Self::U64 | Self::I64 | Self::F64 => 64,
        }
    }

    /// Whether this is a floating-point storage type.
    #[must_use]
    pub const fn is_float(self) -> bool {
        matches!(self, Self::F32 | Self::F64)
    }

    /// Whether integer values of this storage are two's complement.
    #[must_use]
    pub const fn is_signed(self) -> bool {
        match self {
            Self::I8 | Self::I16 | Self::I24 | Self::I32 | Self::I64 => true,
            Self::U20 => U20_BIT19_SIGN_EXTENDS,
            _ => false,
        }
    }

    /// Inclusive raw range of an integer storage (without a bitfield).
    #[must_use]
    pub const fn int_range(self) -> (i128, i128) {
        let bits = self.value_bits();
        if self.is_signed() {
            (-(1_i128 << (bits - 1)), (1_i128 << (bits - 1)) - 1)
        } else {
            (0, (1_i128 << bits) - 1)
        }
    }
}

impl fmt::Display for Storage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// Byte order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Endian {
    /// Most significant byte first.
    Big,
    /// Least significant byte first.
    Little,
}

/// Table layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Layout {
    /// Each cell is its own value.
    #[default]
    Default,
    /// Bosch Motronic "subtract" layout (see the module docs).
    BoschSubtract,
}

/// A bitfield within the stored value: `raw = (stored & mask) >> shift`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Bitfield {
    /// Bits of the stored value that belong to the field.
    pub mask: u64,
    /// Right shift after masking.
    pub shift: u32,
}

impl Bitfield {
    /// A bitfield whose shift is `shift`, or the mask's trailing zeros when
    /// `None`.
    #[must_use]
    pub const fn new(mask: u64, shift: Option<u32>) -> Self {
        let shift = match shift {
            Some(shift) => shift,
            None => mask.trailing_zeros(),
        };
        Self { mask, shift }
    }
}

/// How cells are stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Encoding {
    /// Storage type.
    pub storage: Storage,
    /// Byte order.
    pub endian: Endian,
    /// Table layout.
    pub layout: Layout,
    /// Optional bitfield within each stored value.
    pub bitfield: Option<Bitfield>,
}

impl Encoding {
    /// Plain cells of `storage` in `endian` order: default layout, no bitfield.
    #[must_use]
    pub const fn new(storage: Storage, endian: Endian) -> Self {
        Self {
            storage,
            endian,
            layout: Layout::Default,
            bitfield: None,
        }
    }

    /// The same encoding with a bitfield.
    #[must_use]
    pub const fn with_bitfield(self, bitfield: Bitfield) -> Self {
        Self {
            bitfield: Some(bitfield),
            ..self
        }
    }

    /// The same encoding with a layout.
    #[must_use]
    pub const fn with_layout(self, layout: Layout) -> Self {
        Self { layout, ..self }
    }
}

/// A raw (pre-conversion) value.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Raw {
    /// An integer value (covers every integer storage, bitfields and Bosch
    /// subtract values up to `2^64`).
    Int(i128),
    /// A binary32 value.
    F32(f32),
    /// A binary64 value.
    F64(f64),
}

impl Raw {
    const fn kind(self) -> &'static str {
        match self {
            Self::Int(_) => "integer",
            Self::F32(_) => "f32",
            Self::F64(_) => "f64",
        }
    }
}

/// Why the reference codec refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ReferenceError {
    /// The cell (or table) does not fit in the input.
    #[error("{width} byte(s) at offset {offset} do not fit in {len} byte(s) of input")]
    ShortInput {
        /// Requested start.
        offset: usize,
        /// Requested length (saturated if it overflowed).
        width: usize,
        /// Available bytes.
        len: usize,
    },
    /// An integer value does not fit the storage type.
    #[error("value {value} does not fit {storage} (range {min}..={max})")]
    OutOfRange {
        /// The value.
        value: i128,
        /// The storage.
        storage: Storage,
        /// Smallest storable value.
        min: i128,
        /// Largest storable value.
        max: i128,
    },
    /// A value does not fit the bitfield.
    #[error("value {value} does not fit bitfield mask {mask:#x} shift {shift}")]
    BitfieldOutOfRange {
        /// The value.
        value: i128,
        /// The bitfield mask.
        mask: u64,
        /// The bitfield shift.
        shift: u32,
    },
    /// The bitfield cannot apply to this storage.
    #[error("invalid bitfield mask {mask:#x} shift {shift} on {storage}: {reason}")]
    InvalidBitfield {
        /// The storage.
        storage: Storage,
        /// The bitfield mask.
        mask: u64,
        /// The bitfield shift.
        shift: u32,
        /// What is wrong.
        reason: &'static str,
    },
    /// The raw value's kind does not match the storage (an integer for float
    /// storage, an `f64` for `f32` storage, ...).
    #[error("{storage} cannot hold a {kind} raw value")]
    KindMismatch {
        /// The storage.
        storage: Storage,
        /// The raw value's kind.
        kind: &'static str,
    },
    /// The encoding combination has no defined meaning.
    #[error("unsupported encoding: {0}")]
    Unsupported(&'static str),
    /// A table cell failed.
    #[error("table cell {index}: {source}")]
    TableCell {
        /// Cell index within the table.
        index: usize,
        /// The cell's error.
        source: Box<ReferenceError>,
    },
}

// ---------------------------------------------------------------------------
// Byte access
// ---------------------------------------------------------------------------

fn short(offset: usize, width: usize, len: usize) -> ReferenceError {
    ReferenceError::ShortInput { offset, width, len }
}

fn cell(bytes: &[u8], offset: usize, width: usize) -> Result<&[u8], ReferenceError> {
    offset
        .checked_add(width)
        .and_then(|end| bytes.get(offset..end))
        .ok_or_else(|| short(offset, width, bytes.len()))
}

fn cell_mut(bytes: &mut [u8], offset: usize, width: usize) -> Result<&mut [u8], ReferenceError> {
    let len = bytes.len();
    offset
        .checked_add(width)
        .and_then(|end| bytes.get_mut(offset..end))
        .ok_or_else(|| short(offset, width, len))
}

/// The low 8 bits of `value`.
const fn low_byte(value: u64) -> u8 {
    // Masked to 0..=255 first, so the cast never drops set bits.
    (value & 0xFF) as u8
}

/// Assembles a cell into its value pattern, one byte at a time.
fn read_pattern(cell: &[u8], encoding: &Encoding) -> Result<u64, ReferenceError> {
    if encoding.storage == Storage::U20 {
        if encoding.endian != Endian::Big {
            return Err(ReferenceError::Unsupported(
                "u20 (MOVI20) is a big-endian SH-2A instruction field; little-endian is not defined",
            ));
        }
        let [c0, c1, c2] = cell else {
            return Err(short(0, 3, cell.len()));
        };
        // imm20[19:16] is the high nibble of c0; c0's low nibble is opcode.
        let high = u64::from(*c0) >> 4;
        return Ok((high << 16) | (u64::from(*c1) << 8) | u64::from(*c2));
    }
    let mut pattern = 0_u64;
    match encoding.endian {
        Endian::Big => {
            for byte in cell {
                pattern = (pattern << 8) | u64::from(*byte);
            }
        }
        Endian::Little => {
            for (index, byte) in cell.iter().enumerate() {
                pattern |= u64::from(*byte) << (8 * index);
            }
        }
    }
    Ok(pattern)
}

/// Splits a value pattern back into the cell, one byte at a time.
fn write_pattern(cell: &mut [u8], encoding: &Encoding, pattern: u64) -> Result<(), ReferenceError> {
    if encoding.storage == Storage::U20 {
        let len = cell.len();
        let [c0, c1, c2] = cell else {
            return Err(short(0, 3, len));
        };
        *c0 = (low_byte(pattern >> 16) << 4) | (*c0 & 0x0F);
        *c1 = low_byte(pattern >> 8);
        *c2 = low_byte(pattern);
        return Ok(());
    }
    let last = cell.len().saturating_sub(1);
    for (index, byte) in cell.iter_mut().enumerate() {
        let shift = match encoding.endian {
            Endian::Big => 8 * (last - index),
            Endian::Little => 8 * index,
        };
        *byte = low_byte(pattern >> shift);
    }
    Ok(())
}

fn check_bitfield(encoding: &Encoding) -> Result<(), ReferenceError> {
    let Some(Bitfield { mask, shift }) = encoding.bitfield else {
        return Ok(());
    };
    let storage = encoding.storage;
    let invalid = |reason| ReferenceError::InvalidBitfield {
        storage,
        mask,
        shift,
        reason,
    };
    let bits = storage.value_bits();
    if storage.is_float() {
        Err(invalid("bitfields apply to integer storage only"))
    } else if mask == 0 {
        Err(invalid("mask is zero"))
    } else if shift >= 64 {
        Err(invalid("shift must be below 64"))
    } else if bits < 64 && (mask >> bits) != 0 {
        Err(invalid("mask has bits outside the stored value"))
    } else {
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Single cells
// ---------------------------------------------------------------------------

/// Decodes the cell at `offset` (default layout only).
///
/// # Errors
///
/// [`ReferenceError::ShortInput`] if the cell does not fit `bytes`;
/// [`ReferenceError::Unsupported`] for [`Layout::BoschSubtract`] (use
/// [`decode_table`]) or little-endian `u20`;
/// [`ReferenceError::InvalidBitfield`] for a bitfield that cannot apply.
pub fn decode(bytes: &[u8], offset: usize, encoding: &Encoding) -> Result<Raw, ReferenceError> {
    if encoding.layout == Layout::BoschSubtract {
        return Err(ReferenceError::Unsupported(
            "a bosch-subtract cell depends on every later cell: use decode_table",
        ));
    }
    decode_cell(bytes, offset, encoding)
}

fn decode_cell(bytes: &[u8], offset: usize, encoding: &Encoding) -> Result<Raw, ReferenceError> {
    check_bitfield(encoding)?;
    let storage = encoding.storage;
    let pattern = read_pattern(cell(bytes, offset, storage.cell_bytes())?, encoding)?;
    if let Some(Bitfield { mask, shift }) = encoding.bitfield {
        return Ok(Raw::Int(i128::from((pattern & mask) >> shift)));
    }
    Ok(match storage {
        // The pattern has exactly 32 bits (4 bytes), so the cast is lossless.
        Storage::F32 => Raw::F32(f32::from_bits((pattern & 0xFFFF_FFFF) as u32)),
        Storage::F64 => Raw::F64(f64::from_bits(pattern)),
        _ => {
            let bits = storage.value_bits();
            let top_bit_set = (pattern >> (bits - 1)) & 1 == 1;
            let unsigned = i128::from(pattern);
            if storage.is_signed() && top_bit_set {
                Raw::Int(unsigned - (1_i128 << bits))
            } else {
                Raw::Int(unsigned)
            }
        }
    })
}

/// Encodes `raw` into the cell at `offset` of `dest` (default layout only).
///
/// Bits of `dest` that the encoding does not own are preserved: bits outside a
/// bitfield's mask, and the opcode nibble of a `u20` cell. On error `dest` is
/// unchanged.
///
/// # Errors
///
/// [`ReferenceError::ShortInput`] if the cell does not fit `dest`;
/// [`ReferenceError::OutOfRange`] / [`ReferenceError::BitfieldOutOfRange`] if
/// the value does not fit; [`ReferenceError::KindMismatch`] for a float value
/// in integer storage (or the reverse, or `f32`/`f64` swapped);
/// [`ReferenceError::Unsupported`] for [`Layout::BoschSubtract`] (use
/// [`encode_table`]) or little-endian `u20`;
/// [`ReferenceError::InvalidBitfield`] for a bitfield that cannot apply.
pub fn encode(
    raw: Raw,
    encoding: &Encoding,
    dest: &mut [u8],
    offset: usize,
) -> Result<(), ReferenceError> {
    if encoding.layout == Layout::BoschSubtract {
        return Err(ReferenceError::Unsupported(
            "a bosch-subtract cell depends on every later cell: use encode_table",
        ));
    }
    encode_cell(raw, encoding, dest, offset)
}

fn encode_cell(
    raw: Raw,
    encoding: &Encoding,
    dest: &mut [u8],
    offset: usize,
) -> Result<(), ReferenceError> {
    check_bitfield(encoding)?;
    let cell = cell_mut(dest, offset, encoding.storage.cell_bytes())?;
    let old = read_pattern(cell, encoding)?;
    let new = pattern_for(raw, encoding, old)?;
    write_pattern(cell, encoding, new)
}

/// The value pattern that stores `raw`, given the cell's current pattern.
fn pattern_for(raw: Raw, encoding: &Encoding, old: u64) -> Result<u64, ReferenceError> {
    let storage = encoding.storage;
    let mismatch = ReferenceError::KindMismatch {
        storage,
        kind: raw.kind(),
    };
    if let Some(Bitfield { mask, shift }) = encoding.bitfield {
        let Raw::Int(value) = raw else {
            return Err(mismatch);
        };
        let field = u64::try_from(value)
            .ok()
            .and_then(|v| v.checked_shl(shift).filter(|s| (s >> shift) == v))
            .filter(|s| (s & !mask) == 0)
            .ok_or(ReferenceError::BitfieldOutOfRange { value, mask, shift })?;
        return Ok((old & !mask) | field);
    }
    match (storage, raw) {
        (Storage::F32, Raw::F32(value)) => Ok(u64::from(value.to_bits())),
        (Storage::F64, Raw::F64(value)) => Ok(value.to_bits()),
        (Storage::F32 | Storage::F64, _) | (_, Raw::F32(_) | Raw::F64(_)) => Err(mismatch),
        (_, Raw::Int(value)) => {
            let (min, max) = storage.int_range();
            let out_of_range = ReferenceError::OutOfRange {
                value,
                storage,
                min,
                max,
            };
            if value < min || value > max {
                return Err(out_of_range);
            }
            // Two's complement: a negative value is stored as value + 2^bits.
            let unsigned = if value < 0 {
                value + (1_i128 << storage.value_bits())
            } else {
                value
            };
            u64::try_from(unsigned).map_err(|_| out_of_range)
        }
    }
}

// ---------------------------------------------------------------------------
// Tables
// ---------------------------------------------------------------------------

/// Checks `count` cells from `offset` fit `len` bytes; returns the cell stride.
fn table_extent(
    offset: usize,
    count: usize,
    storage: Storage,
    len: usize,
) -> Result<usize, ReferenceError> {
    let stride = storage.cell_bytes();
    let width = count.saturating_mul(stride);
    match offset.checked_add(width) {
        Some(end) if end <= len => Ok(stride),
        _ => Err(short(offset, width, len)),
    }
}

/// `2^(8W)` for a Bosch-subtract table of `W`-byte integer cells.
fn bosch_total(storage: Storage) -> Result<i128, ReferenceError> {
    if storage.is_float() || storage == Storage::U20 {
        return Err(ReferenceError::Unsupported(
            "bosch-subtract is defined for 1/2/3/4/8-byte integer storage only",
        ));
    }
    // For these storages value_bits() is exactly 8 * cell_bytes() = 8W.
    Ok(1_i128 << storage.value_bits())
}

fn in_cell(index: usize) -> impl FnOnce(ReferenceError) -> ReferenceError {
    move |source| ReferenceError::TableCell {
        index,
        source: Box::new(source),
    }
}

/// Decodes a table of `count` consecutive cells starting at `offset` (cell
/// `i` at `offset + i * cell_bytes`), in either layout.
///
/// # Errors
///
/// [`ReferenceError::ShortInput`] if the table does not fit `bytes` (checked
/// before anything is read); [`ReferenceError::Unsupported`] for a
/// Bosch-subtract table of float or `u20` storage; otherwise as [`decode`],
/// wrapped in [`ReferenceError::TableCell`].
pub fn decode_table(
    bytes: &[u8],
    offset: usize,
    count: usize,
    encoding: &Encoding,
) -> Result<Vec<Raw>, ReferenceError> {
    let stride = table_extent(offset, count, encoding.storage, bytes.len())?;
    let total = match encoding.layout {
        Layout::Default => None,
        Layout::BoschSubtract => Some(bosch_total(encoding.storage)?),
    };
    let mut cells = Vec::with_capacity(count);
    for index in 0..count {
        let raw = decode_cell(bytes, offset + index * stride, encoding).map_err(in_cell(index))?;
        cells.push(raw);
    }
    let Some(total) = total else {
        return Ok(cells);
    };
    // value[i] = 2^(8W) - (s[i] + ... + s[N-1]), accumulated from the end.
    let mut running = total;
    for index in (0..count).rev() {
        let stored = match cells.get(index) {
            Some(Raw::Int(stored)) => *stored,
            _ => {
                return Err(in_cell(index)(ReferenceError::Unsupported(
                    "bosch-subtract cell is not an integer",
                )));
            }
        };
        running = running.checked_sub(stored).ok_or_else(|| {
            in_cell(index)(ReferenceError::Unsupported("bosch-subtract sum overflows"))
        })?;
        if let Some(slot) = cells.get_mut(index) {
            *slot = Raw::Int(running);
        }
    }
    Ok(cells)
}

/// Encodes `values` as a table of consecutive cells starting at `offset`, in
/// either layout. All or nothing: on error `dest` is unchanged.
///
/// # Errors
///
/// [`ReferenceError::ShortInput`] if the table does not fit `dest`;
/// [`ReferenceError::Unsupported`] for a Bosch-subtract table of float or
/// `u20` storage; [`ReferenceError::TableCell`] wrapping the first cell that
/// fails as [`encode`] does, where for Bosch subtract the checked value is the
/// derived stored cell (`2^(8W) - value[N-1]`, or `value[i+1] - value[i]`).
pub fn encode_table(
    values: &[Raw],
    encoding: &Encoding,
    dest: &mut [u8],
    offset: usize,
) -> Result<(), ReferenceError> {
    let count = values.len();
    let stride = table_extent(offset, count, encoding.storage, dest.len())?;
    let region = cell_mut(dest, offset, count * stride)?;
    let stored: Vec<Raw> = match encoding.layout {
        Layout::Default => values.to_vec(),
        Layout::BoschSubtract => {
            let total = bosch_total(encoding.storage)?;
            let mut ints = Vec::with_capacity(count);
            for (index, value) in values.iter().enumerate() {
                match value {
                    Raw::Int(value) => ints.push(*value),
                    other => {
                        return Err(in_cell(index)(ReferenceError::KindMismatch {
                            storage: encoding.storage,
                            kind: other.kind(),
                        }));
                    }
                }
            }
            let mut stored = Vec::with_capacity(count);
            for (index, value) in ints.iter().enumerate() {
                let next = match ints.get(index + 1) {
                    Some(next) => *next,
                    None => total,
                };
                let diff = next.checked_sub(*value).ok_or_else(|| {
                    in_cell(index)(ReferenceError::Unsupported(
                        "bosch-subtract difference overflows",
                    ))
                })?;
                stored.push(Raw::Int(diff));
            }
            stored
        }
    };
    let mut scratch = region.to_vec();
    for (index, raw) in stored.into_iter().enumerate() {
        encode_cell(raw, encoding, &mut scratch, index * stride).map_err(in_cell(index))?;
    }
    region.copy_from_slice(&scratch);
    Ok(())
}
