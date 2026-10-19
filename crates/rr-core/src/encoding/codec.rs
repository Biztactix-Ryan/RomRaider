//! The cell codec (US-RR-19-8): one cell's bytes to a [`Raw`] value and back.
//!
//! Every conversion goes through three steps, each in one place:
//!
//! 1. **cell pattern**: the cell's `8 * cell_bytes` bits as an unsigned
//!    number, in the encoding's byte order ([`read_cell`] / [`write_cell`]);
//! 2. **value pattern**: the stored value's [`value_bits`](Storage::value_bits)
//!    bits. The same as the cell pattern except for `U20`, whose 20 immediate
//!    bits are split around the `MOVI20` opcode nibble ([`value_of_cell`] /
//!    [`cell_of_value`]);
//! 3. **raw value**: the value pattern read as the storage's number kind,
//!    two's complement for signed storage, or the bitfield's bits of it.
//!
//! Encoding runs the steps backwards, read-modify-write: the cell's current
//! bytes are read first, so the bits the encoding does not own (outside a
//! bitfield's mask, the opcode nibble of a `U20` cell) are written back
//! unchanged. Nothing is written unless the whole value fits.
//!
//! The single-cell entry points ([`Encoding::decode`],
//! [`Encoding::encode_into`]) refuse a whole-table layout; whole tables, in
//! either layout, go through the `table` module, which uses the same cell
//! steps.
//!
//! Floats are converted bit-exactly (`from_bits` / `to_bits`): NaN payloads,
//! `-0.0` and subnormals come back as they were stored.

use core::fmt;

use super::{BitField, Encoding, EncodingError, Endian, Layout, Storage};

/// A raw (pre-conversion) value: what one cell holds.
///
/// `Int` holds every integer storage's full range exactly (`u64::MAX` and
/// `i64::MIN` included) and leaves room for table layouts whose values reach
/// `2^64` (Bosch subtract). Floats keep their storage's width, so a value
/// always says which float storage it fits.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Raw {
    /// An integer value.
    Int(i128),
    /// An IEEE 754 binary32 value.
    F32(f32),
    /// An IEEE 754 binary64 value.
    F64(f64),
}

impl Raw {
    /// Which kind of value this is.
    #[must_use]
    pub const fn kind(self) -> RawKind {
        match self {
            Self::Int(_) => RawKind::Int,
            Self::F32(_) => RawKind::F32,
            Self::F64(_) => RawKind::F64,
        }
    }
}

/// The kind of a [`Raw`] value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RawKind {
    /// [`Raw::Int`].
    Int,
    /// [`Raw::F32`].
    F32,
    /// [`Raw::F64`].
    F64,
}

impl RawKind {
    /// Lower-case name: `integer`, `f32` or `f64`.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Int => "integer",
            Self::F32 => "f32",
            Self::F64 => "f64",
        }
    }
}

/// The name, as [`RawKind::name`].
impl fmt::Display for RawKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

impl Encoding {
    /// Decodes the cell at the start of `bytes` (its first
    /// [`cell_bytes`](Storage::cell_bytes) bytes; any further bytes are not
    /// read).
    ///
    /// For [`Storage::U20`] the cell is the 3 bytes from the `MOVI20`
    /// instruction's second byte: `(c0 >> 4) * 65536 + c1 * 256 + c2`.
    ///
    /// # Errors
    ///
    /// * [`EncodingError::WholeTableLayout`] for [`Layout::BoschSubtract`],
    ///   whose cells have no value on their own;
    /// * [`EncodingError::ShortInput`] if `bytes` is shorter than the cell.
    pub fn decode(self, bytes: &[u8]) -> Result<Raw, EncodingError> {
        self.check_single_cell()?;
        self.decode_cell(bytes)
    }

    /// [`decode`](Self::decode) of one cell's stored value, whatever the
    /// layout: for [`Layout::BoschSubtract`] the stored cell `s[i]`, not the
    /// table value (the `table` module combines them).
    pub(super) fn decode_cell(self, bytes: &[u8]) -> Result<Raw, EncodingError> {
        let width = self.storage.cell_bytes();
        let cell = bytes.get(..width).ok_or(EncodingError::ShortInput {
            needed: width,
            len: bytes.len(),
        })?;
        let value = value_of_cell(self.storage, read_cell(self.endian, cell));
        Ok(self.raw_of_value(value))
    }

    /// Encodes `raw` into the cell at the start of `bytes`, keeping the bits
    /// the encoding does not own: those outside the bitfield's mask, and the
    /// opcode nibble (low nibble of the first byte) of a [`Storage::U20`]
    /// cell. Bytes after the cell are not touched. On error nothing is
    /// written.
    ///
    /// # Errors
    ///
    /// * [`EncodingError::WholeTableLayout`] for [`Layout::BoschSubtract`];
    /// * [`EncodingError::ShortInput`] if `bytes` is shorter than the cell;
    /// * [`EncodingError::KindMismatch`] if `raw` is not the storage's kind
    ///   (an integer for integer storage, `F32` for `F32`, `F64` for `F64`);
    /// * [`EncodingError::OutOfRange`] if an integer is outside the
    ///   storage's range, or [`EncodingError::BitFieldOutOfRange`] if it does
    ///   not fit the bitfield: the integer is stored exactly when
    ///   [`stored_range`](Self::stored_range) contains it. Values are never
    ///   truncated.
    pub fn encode_into(self, raw: Raw, bytes: &mut [u8]) -> Result<(), EncodingError> {
        self.check_single_cell()?;
        self.encode_cell(raw, bytes)
    }

    /// [`encode_into`](Self::encode_into) of one cell's stored value,
    /// whatever the layout: for [`Layout::BoschSubtract`] `raw` is the stored
    /// cell `s[i]`, not the table value (the `table` module derives it).
    pub(super) fn encode_cell(self, raw: Raw, bytes: &mut [u8]) -> Result<(), EncodingError> {
        let width = self.storage.cell_bytes();
        let len = bytes.len();
        let cell = bytes
            .get_mut(..width)
            .ok_or(EncodingError::ShortInput { needed: width, len })?;
        let old_cell = read_cell(self.endian, cell);
        let old_value = value_of_cell(self.storage, old_cell);
        let new_value = self.value_of_raw(raw, old_value)?;
        write_cell(
            self.endian,
            cell,
            cell_of_value(self.storage, new_value, old_cell),
        );
        Ok(())
    }

    /// Encodes `raw` into a new cell of [`cell_bytes`](Storage::cell_bytes)
    /// bytes whose bits outside the encoding (see
    /// [`encode_into`](Self::encode_into)) are zero.
    ///
    /// # Errors
    ///
    /// As [`encode_into`](Self::encode_into), except that the cell always
    /// fits.
    pub fn encode(self, raw: Raw) -> Result<Vec<u8>, EncodingError> {
        let mut cell = vec![0; self.storage.cell_bytes()];
        self.encode_into(raw, &mut cell)?;
        Ok(cell)
    }

    /// Refuses a layout in which one cell has no value by itself.
    const fn check_single_cell(self) -> Result<(), EncodingError> {
        match self.layout {
            Layout::Default => Ok(()),
            Layout::BoschSubtract => Err(EncodingError::WholeTableLayout {
                layout: self.layout,
            }),
        }
    }

    /// The raw value a value pattern holds.
    fn raw_of_value(self, value: u64) -> Raw {
        match (self.storage, self.bitfield) {
            // An F32 value pattern has 32 bits, so the cast keeps every bit.
            (Storage::F32, _) => Raw::F32(f32::from_bits(value as u32)),
            (Storage::F64, _) => Raw::F64(f64::from_bits(value)),
            (_, Some(field)) => Raw::Int(field.extract(value)),
            (storage, None) => Raw::Int(sign_extend(
                value,
                storage.value_bits(),
                storage.is_signed(),
            )),
        }
    }

    /// The value pattern that stores `raw`, given the cell's current one.
    fn value_of_raw(self, raw: Raw, old: u64) -> Result<u64, EncodingError> {
        let storage = self.storage;
        let mismatch = EncodingError::KindMismatch {
            storage,
            kind: raw.kind(),
        };
        match raw {
            Raw::Int(value) => {
                // The one range check: the same range `stored_range` reports.
                let Some(range) = self.stored_range().as_int() else {
                    return Err(mismatch);
                };
                if !range.contains(value) {
                    return Err(match self.bitfield {
                        Some(field) => EncodingError::BitFieldOutOfRange {
                            value,
                            mask: field.mask,
                            shift: field.shift,
                            signed: field.signed,
                        },
                        None => EncodingError::OutOfRange {
                            value,
                            storage,
                            min: range.min(),
                            max: range.max(),
                        },
                    });
                }
                Ok(match self.bitfield {
                    Some(field) => field.insert(value, old),
                    // Two's complement: the low 64 bits of the value. Bits
                    // above the stored value are never written (write_cell
                    // keeps only the cell's bytes, cell_of_value only a
                    // U20's 20 bits).
                    None => value as u64,
                })
            }
            Raw::F32(value) if matches!(storage, Storage::F32) => Ok(u64::from(value.to_bits())),
            Raw::F64(value) if matches!(storage, Storage::F64) => Ok(value.to_bits()),
            Raw::F32(_) | Raw::F64(_) => Err(mismatch),
        }
    }
}

impl BitField {
    /// Bits in the field after the shift: one more than the index of its top
    /// bit (the sign bit of a signed field).
    const fn width(self) -> u32 {
        self.reach_bits() - self.shift
    }

    /// The field's raw value in a value pattern.
    const fn extract(self, value: u64) -> i128 {
        sign_extend((value & self.mask) >> self.shift, self.width(), self.signed)
    }

    /// `old` with the field set to `value`, which is in the field's
    /// [`raw_range`](BitField::raw_range) (so its bits, moved to the field's
    /// position, are inside the mask).
    const fn insert(self, value: i128, old: u64) -> u64 {
        // Two's complement in `width` bits, moved to the field's position.
        let field = (value as u64 & low_bits(self.width())) << self.shift;
        // The two parts have no bit in common (`field` is inside the mask),
        // so `+` is the bitwise union.
        (old & !self.mask) + field
    }
}

/// A mask of the low `bits` bits (`1..=64`).
const fn low_bits(bits: u32) -> u64 {
    u64::MAX >> (u64::BITS - bits)
}

/// The `bits`-bit pattern `value` as a number: two's complement if `signed`.
const fn sign_extend(value: u64, bits: u32, signed: bool) -> i128 {
    let unsigned = value as i128;
    if signed && (value >> (bits - 1)) & 1 == 1 {
        unsigned - (1_i128 << bits)
    } else {
        unsigned
    }
}

/// The cell's bits as one unsigned number in `endian` order (`cell` is at
/// most 8 bytes).
fn read_cell(endian: Endian, cell: &[u8]) -> u64 {
    let mut word = [0_u8; 8];
    match endian {
        Endian::Big => {
            word[8 - cell.len()..].copy_from_slice(cell);
            u64::from_be_bytes(word)
        }
        Endian::Little => {
            word[..cell.len()].copy_from_slice(cell);
            u64::from_le_bytes(word)
        }
    }
}

/// Writes the low `cell.len()` bytes of `pattern` into `cell` in `endian`
/// order.
fn write_cell(endian: Endian, cell: &mut [u8], pattern: u64) {
    let width = cell.len();
    match endian {
        Endian::Big => cell.copy_from_slice(&pattern.to_be_bytes()[8 - width..]),
        Endian::Little => cell.copy_from_slice(&pattern.to_le_bytes()[..width]),
    }
}

/// `MOVI20` cell bits holding `imm20[19:16]`: the high nibble of the first
/// byte.
const U20_HIGH_CELL: u64 = 0xF0_0000;
/// `MOVI20` cell bits that are opcode, not immediate: the low nibble of the
/// first byte.
const U20_OPCODE_CELL: u64 = 0x0F_0000;
/// `MOVI20` cell bits holding `imm20[15:0]`: the second and third bytes.
const U20_LOW: u64 = 0xFFFF;

// The parts below are combined with `+`: they never share a bit, so the sum
// is the bitwise union.

/// The value pattern in a cell pattern.
const fn value_of_cell(storage: Storage, cell: u64) -> u64 {
    match storage {
        Storage::U20 => ((cell & U20_HIGH_CELL) >> 4) + (cell & U20_LOW),
        _ => cell,
    }
}

/// The cell pattern storing `value`, keeping the bits of `old` (the cell's
/// current pattern) that are not part of the value.
const fn cell_of_value(storage: Storage, value: u64, old: u64) -> u64 {
    match storage {
        Storage::U20 => {
            (old & U20_OPCODE_CELL) + ((value << 4) & U20_HIGH_CELL) + (value & U20_LOW)
        }
        _ => value,
    }
}

#[cfg(test)]
mod tests;
