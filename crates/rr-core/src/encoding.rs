//! Encoding (concept C3): how a raw value is stored in bytes.
//!
//! An [`Encoding`] is `{ storage, endian, bitfield, layout }`:
//!
//! * [`Storage`] — the cell's width, signedness and number kind: `U8`..`U64`,
//!   `I8`..`I64`, `U20` (the SH-2A `MOVI20` immediate), `U24` / `I24`, `F32` /
//!   `F64`.
//! * [`Endian`] — byte order of a multi-byte cell.
//! * [`BitField`] — optionally, only some bits of the stored value are the
//!   field: `raw = (stored & mask) >> shift`, optionally sign-extended.
//! * [`Layout`] — how cells relate to each other: each cell on its own
//!   ([`Layout::Default`]) or Bosch Motronic "subtract"
//!   ([`Layout::BoschSubtract`]).
//!
//! One encoding is shared by ROM table cells, logger parameters, device
//! fields and protocol headers (ARCHITECTURE.md C3). The type holds no
//! static or global state: two encodings with different byte orders are
//! independent values and can be used from different threads at once
//! (MF-04).
//!
//! # Fail closed
//!
//! An encoding can only be built through [`Encoding::new`], which refuses
//! every combination that has no defined meaning (see [`EncodingError`]), so
//! a value of type `Encoding` is always one the codec can handle. Strings from
//! definitions are parsed by [`legacy`], whose unknown or unsupported values
//! become [`Error`](crate::diag::Severity::Error) diagnostics, never a default
//! and never a zero value (MF-02).
//!
//! # `u20` bit 19
//!
//! Whether bit 19 of a `MOVI20` immediate sign-extends is decided in exactly
//! one place, [`U20_BIT19_SIGN_EXTENDS`]. Every signedness and range helper
//! here reads it.
//!
//! # Bytes
//!
//! [`Encoding::decode`] reads one cell from bytes into a [`Raw`] value and
//! [`Encoding::encode_into`] writes one back, read-modify-write, so bits the
//! encoding does not own (outside a bitfield's mask, the opcode nibble of a
//! `MOVI20` cell) keep their values. Both return a [`Result`]: a cell that
//! does not fit the bytes, a value that does not fit the storage and a value
//! of the wrong kind are errors, never a panic, a zero or a truncated value.
//! The cell codec is in the private `codec` module.
//!
//! # Tables
//!
//! [`Encoding::decode_table`] and [`Encoding::encode_table`] read and write a
//! run of consecutive cells in either [`Layout`], so callers have one path
//! for every table. Encoding a table is all or nothing. A
//! [`Layout::BoschSubtract`] cell has no value on its own: the single-cell
//! [`decode`](Encoding::decode) / [`encode_into`](Encoding::encode_into)
//! refuse it ([`EncodingError::WholeTableLayout`]), and the table codec
//! applies the legacy transform (`maps/DataCell.java:193-231`), refusing a
//! table whose derived cells do not fit where legacy silently wrapped. The
//! table codec is in the private `table` module.
//!
//! # Special floats (EC-07)
//!
//! Floats decode and encode bit-exactly, NaN payloads included. A decoded
//! float carries its [`FloatClass`] ([`Raw::float_class`]), and an edit
//! layer refuses NaN, `±Inf` and subnormal values unless the user explicitly
//! allowed them ([`SpecialFloats`], [`Raw::check_edit`],
//! [`Encoding::edit_into`]). The classes and the gate are in the `float`
//! module.
//!
//! # Ranges (MF-15)
//!
//! [`Encoding::raw_range`] is the range of raw values one cell can hold,
//! derived from the encoding alone: the storage's integer range, a
//! bitfield's (`-2^(n-1)..=2^(n-1)-1` signed, `0..=mask >> shift` unsigned),
//! or a float width's finite range `-MAX..=MAX`, never `0` or the smallest
//! positive float as a minimum (legacy swapped its signed and unsigned float
//! ranges, `maps/DataCell.java:140-147`). The cell codec checks integers
//! against that same range ([`RawRange`], [`IntRange`]), so the two agree by
//! construction. The ranges are in the private `range` module.

mod codec;
mod float;
pub mod legacy;
mod range;
mod table;

pub use codec::{Raw, RawKind};
pub use float::{EditOperand, FloatClass, SpecialFloats};
pub use range::{IntRange, RawRange};

use core::fmt;

use crate::diag::{Code, Diagnostic, Provenance, codes};

/// THE `u20` (`MOVI20`) bit-19 decision point.
///
/// `true`: bit 19 of the 20-bit immediate **sign-extends**, giving
/// `-0x80000..=0x7FFFF`, as the SH-2A CPU and legacy do (legacy treats
/// `movi20` as signed: `RomAttributeParser.java:73-81` and `getMovi20`,
/// `RomAttributeParser.java:181-191`). `false` would read it unsigned
/// (`0..=0xFFFFF`), as the name `u20` suggests.
///
/// This matches the independent reference codec
/// (`rr_testkit::reference::U20_BIT19_SIGN_EXTENDS`, also `true`).
///
/// Decided by the maintainer in US-RR-19, to match legacy.
/// [`Storage::is_signed`], [`Storage::int_range`] and the raw ranges
/// ([`Encoding::raw_range`]) follow this constant, so flipping it changes
/// every `u20` helper at once.
pub const U20_BIT19_SIGN_EXTENDS: bool = true;

/// Storage type of one cell: width, signedness and number kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Storage {
    /// Unsigned 8-bit.
    U8,
    /// Unsigned 16-bit.
    U16,
    /// 20-bit SH-2A `MOVI20` immediate in a 3-byte cell (see
    /// [`U20_BIT19_SIGN_EXTENDS`]).
    U20,
    /// Unsigned 24-bit.
    U24,
    /// Unsigned 32-bit.
    U32,
    /// Unsigned 64-bit.
    U64,
    /// Signed (two's complement) 8-bit.
    I8,
    /// Signed (two's complement) 16-bit.
    I16,
    /// Signed (two's complement) 24-bit.
    I24,
    /// Signed (two's complement) 32-bit.
    I32,
    /// Signed (two's complement) 64-bit.
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

    /// Canonical lower-case name: `u8`, `i24`, `f64`, ...
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

    /// Bytes one cell occupies (`U20`: 3, the immediate's bytes).
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

    /// Bits in the stored value (`U20`: 20, otherwise `8 * cell_bytes`).
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

    /// `true` for the IEEE 754 storages, [`F32`](Self::F32) and
    /// [`F64`](Self::F64).
    #[must_use]
    pub const fn is_float(self) -> bool {
        matches!(self, Self::F32 | Self::F64)
    }

    /// `true` for the integer storages (everything but the floats).
    #[must_use]
    pub const fn is_integer(self) -> bool {
        !self.is_float()
    }

    /// `true` if the stored integer is two's complement: the `I*` storages,
    /// and [`U20`](Self::U20) only if [`U20_BIT19_SIGN_EXTENDS`]. Floats are
    /// not integers and report `false`; their sign is part of the IEEE value.
    #[must_use]
    pub const fn is_signed(self) -> bool {
        match self {
            Self::I8 | Self::I16 | Self::I24 | Self::I32 | Self::I64 => true,
            Self::U20 => U20_BIT19_SIGN_EXTENDS,
            Self::U8 | Self::U16 | Self::U24 | Self::U32 | Self::U64 | Self::F32 | Self::F64 => {
                false
            }
        }
    }

    /// Inclusive raw range `(min, max)` of an integer storage (without a
    /// bitfield), or `None` for a float storage.
    #[must_use]
    pub const fn int_range(self) -> Option<(i128, i128)> {
        if self.is_float() {
            return None;
        }
        let bits = self.value_bits();
        Some(if self.is_signed() {
            (-(1_i128 << (bits - 1)), (1_i128 << (bits - 1)) - 1)
        } else {
            (0, (1_i128 << bits) - 1)
        })
    }
}

/// The canonical name, as [`Storage::name`].
impl fmt::Display for Storage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// Byte order of a multi-byte cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Endian {
    /// Most significant byte first.
    Big,
    /// Least significant byte first.
    Little,
}

impl Endian {
    /// Lower-case name: `big` or `little`.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Big => "big",
            Self::Little => "little",
        }
    }
}

/// The name, as [`Endian::name`].
impl fmt::Display for Endian {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// How the cells of a table relate to each other.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Layout {
    /// Each cell is its own value.
    #[default]
    Default,
    /// Bosch Motronic "subtract" layout: a whole-table layout in which each
    /// value is `2^(8W)` minus the sum of the stored cells from it to the end
    /// (`W`: the cell width in bytes; legacy `DataCell.java:193-207`).
    /// Defined for integer storages of 1, 2, 3, 4 or 8 bytes only. Decoded
    /// and encoded by [`Encoding::decode_table`] /
    /// [`Encoding::encode_table`].
    BoschSubtract,
}

impl Layout {
    /// Lower-case name: `default` or `bosch-subtract`.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::BoschSubtract => "bosch-subtract",
        }
    }
}

/// The name, as [`Layout::name`].
impl fmt::Display for Layout {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// A field inside the stored value: `raw = (stored & mask) >> shift`,
/// sign-extended from the field's top bit when `signed`.
///
/// Built by [`BitField::new`], which checks the field on its own; whether it
/// fits a storage is checked by [`Encoding::new`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BitField {
    mask: u64,
    shift: u32,
    signed: bool,
}

impl BitField {
    /// A bitfield of the bits in `mask`, shifted right by `shift` (the mask's
    /// trailing zeros when `None`, i.e. the field's lowest bit becomes bit 0).
    ///
    /// # Errors
    ///
    /// [`EncodingError::InvalidBitField`] when:
    ///
    /// * `mask` is zero ([`BitFieldProblem::ZeroMask`]);
    /// * `shift` is above the mask's lowest set bit, so the shift would drop
    ///   field bits and the value could not be written back
    ///   ([`BitFieldProblem::ShiftDropsBits`]);
    /// * `signed` and the mask is not one contiguous run of bits — a sign bit
    ///   is only meaningful at the top of a contiguous two's complement field
    ///   ([`BitFieldProblem::SignedNotContiguous`]);
    /// * `signed` and the field is a single bit, which leaves no value bits
    ///   ([`BitFieldProblem::SignedTooNarrow`]).
    pub const fn new(mask: u64, shift: Option<u32>, signed: bool) -> Result<Self, EncodingError> {
        let lowest = mask.trailing_zeros();
        let shift = match shift {
            Some(shift) => shift,
            None => lowest,
        };
        let problem = if mask == 0 {
            Some(BitFieldProblem::ZeroMask)
        } else if shift > lowest {
            Some(BitFieldProblem::ShiftDropsBits)
        } else if signed && !is_contiguous(mask) {
            Some(BitFieldProblem::SignedNotContiguous)
        } else if signed && mask.count_ones() < 2 {
            Some(BitFieldProblem::SignedTooNarrow)
        } else {
            None
        };
        match problem {
            Some(problem) => Err(EncodingError::InvalidBitField {
                mask,
                shift,
                problem,
            }),
            None => Ok(Self {
                mask,
                shift,
                signed,
            }),
        }
    }

    /// The bits of the stored value that belong to the field.
    #[must_use]
    pub const fn mask(self) -> u64 {
        self.mask
    }

    /// Right shift applied after masking.
    #[must_use]
    pub const fn shift(self) -> u32 {
        self.shift
    }

    /// `true` if the field is two's complement (sign-extended from its top
    /// bit).
    #[must_use]
    pub const fn is_signed(self) -> bool {
        self.signed
    }

    /// Bits in the stored value the mask reaches: one more than the index of
    /// its highest set bit. The mask fits a storage when this is at most the
    /// storage's [`value_bits`](Storage::value_bits).
    #[must_use]
    pub const fn reach_bits(self) -> u32 {
        u64::BITS - self.mask.leading_zeros()
    }
}

/// `true` if the set bits of a non-zero `mask` form one run.
const fn is_contiguous(mask: u64) -> bool {
    mask.leading_zeros() + mask.count_ones() + mask.trailing_zeros() == u64::BITS
}

/// How a storage is encoded: storage type, byte order, optional bitfield and
/// table layout. Always a combination the codec supports (see
/// [`Encoding::new`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Encoding {
    storage: Storage,
    endian: Endian,
    bitfield: Option<BitField>,
    layout: Layout,
}

impl Encoding {
    /// Checks and builds an encoding.
    ///
    /// # Errors
    ///
    /// * [`EncodingError::UnsupportedCombination`] for a little-endian
    ///   [`Storage::U20`] (`MOVI20` is a big-endian SH-2A instruction field)
    ///   and for [`Layout::BoschSubtract`] on a float or `U20` storage (the
    ///   layout's `2^(8W)` is only defined for whole-byte integers).
    /// * [`EncodingError::InvalidBitField`] for a bitfield on a float storage
    ///   ([`BitFieldProblem::FloatStorage`]) or one whose mask reaches past
    ///   the stored value ([`BitFieldProblem::OutsideStorage`]).
    pub const fn new(
        storage: Storage,
        endian: Endian,
        bitfield: Option<BitField>,
        layout: Layout,
    ) -> Result<Self, EncodingError> {
        let is_u20 = matches!(storage, Storage::U20);
        if is_u20 && matches!(endian, Endian::Little) {
            return Err(EncodingError::UnsupportedCombination {
                storage,
                problem: CombinationProblem::U20LittleEndian,
            });
        }
        if matches!(layout, Layout::BoschSubtract) && (is_u20 || storage.is_float()) {
            return Err(EncodingError::UnsupportedCombination {
                storage,
                problem: CombinationProblem::BoschSubtractStorage,
            });
        }
        if let Some(field) = bitfield {
            let problem = if storage.is_float() {
                Some(BitFieldProblem::FloatStorage)
            } else if field.reach_bits() > storage.value_bits() {
                Some(BitFieldProblem::OutsideStorage {
                    storage_bits: storage.value_bits(),
                })
            } else {
                None
            };
            if let Some(problem) = problem {
                return Err(EncodingError::InvalidBitField {
                    mask: field.mask,
                    shift: field.shift,
                    problem,
                });
            }
        }
        Ok(Self {
            storage,
            endian,
            bitfield,
            layout,
        })
    }

    /// The storage type.
    #[must_use]
    pub const fn storage(self) -> Storage {
        self.storage
    }

    /// The byte order.
    #[must_use]
    pub const fn endian(self) -> Endian {
        self.endian
    }

    /// The bitfield, if only part of the stored value is the field.
    #[must_use]
    pub const fn bitfield(self) -> Option<BitField> {
        self.bitfield
    }

    /// The table layout.
    #[must_use]
    pub const fn layout(self) -> Layout {
        self.layout
    }
}

/// What is wrong with a [`BitField`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BitFieldProblem {
    /// The mask has no bits set.
    ZeroMask,
    /// The shift is above the mask's lowest set bit, so it would drop field
    /// bits.
    ShiftDropsBits,
    /// A signed field whose mask is not one contiguous run of bits.
    SignedNotContiguous,
    /// A signed field of a single bit.
    SignedTooNarrow,
    /// A bitfield on a float storage.
    FloatStorage,
    /// The mask has bits above the stored value's `storage_bits`.
    OutsideStorage {
        /// Bits in the stored value.
        storage_bits: u32,
    },
}

impl fmt::Display for BitFieldProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::ZeroMask => f.write_str("the mask is zero"),
            Self::ShiftDropsBits => {
                f.write_str("the shift is above the mask's lowest bit and would drop field bits")
            }
            Self::SignedNotContiguous => f.write_str("a signed field needs a contiguous mask"),
            Self::SignedTooNarrow => f.write_str("a signed field needs at least 2 bits"),
            Self::FloatStorage => f.write_str("bitfields apply to integer storage only"),
            Self::OutsideStorage { storage_bits } => write!(
                f,
                "the mask has bits outside the {storage_bits}-bit stored value"
            ),
        }
    }
}

/// Why a storage type string was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StorageProblem {
    /// The string names no storage type.
    Unknown,
    /// The string is empty.
    Empty,
    /// Legacy's `movi20s` (a `MOVI20` immediate shifted left by 8), which is
    /// not supported.
    Movi20s,
    /// A bare number: legacy's internal storage code (a byte count, or the
    /// magic 20 / 28 / 99), which is ambiguous about signedness.
    NumericCode,
}

impl fmt::Display for StorageProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Unknown => "not a supported storage type",
            Self::Empty => "the storage type is empty",
            Self::Movi20s => "movi20s (shifted MOVI20 immediate) is not supported",
            Self::NumericCode => {
                "a numeric storage code is ambiguous; name the type (uint16, int8, float, ...)"
            }
        })
    }
}

/// Why a storage/endian/layout combination was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CombinationProblem {
    /// `U20` with little-endian byte order.
    U20LittleEndian,
    /// Bosch-subtract layout on a float or `U20` storage.
    BoschSubtractStorage,
}

impl fmt::Display for CombinationProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::U20LittleEndian => {
                "MOVI20 is a big-endian SH-2A instruction field; little-endian is not defined"
            }
            Self::BoschSubtractStorage => {
                "bosch-subtract is defined for 1/2/3/4/8-byte integer storage only"
            }
        })
    }
}

/// Why an encoding (or a definition string for one) was refused.
///
/// Every variant is an error: [`code`](Self::code) is the registry code it is
/// reported with, and [`to_diagnostic`](Self::to_diagnostic) makes the
/// [`Error`](crate::diag::Severity::Error) diagnostic.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum EncodingError {
    /// A storage type string that names no supported storage type
    /// ([`codes::UNSUPPORTED_STORAGE_TYPE`]).
    UnsupportedStorage {
        /// The string, as written.
        text: String,
        /// Why it was refused.
        problem: StorageProblem,
    },
    /// A supported storage type in a combination that has no defined meaning
    /// ([`codes::UNSUPPORTED_STORAGE_TYPE`]).
    UnsupportedCombination {
        /// The storage type.
        storage: Storage,
        /// What is wrong with the combination.
        problem: CombinationProblem,
    },
    /// An endianness string that is neither `big` nor `little`
    /// ([`codes::UNKNOWN_ENDIAN`]).
    UnknownEndian {
        /// The string, as written.
        text: String,
    },
    /// A table layout string that names no layout ([`codes::UNKNOWN_LAYOUT`]).
    UnknownLayout {
        /// The string, as written.
        text: String,
    },
    /// A bitfield that cannot apply ([`codes::INVALID_BITFIELD`]).
    InvalidBitField {
        /// The mask.
        mask: u64,
        /// The effective shift.
        shift: u32,
        /// What is wrong.
        problem: BitFieldProblem,
    },
    /// A bitfield mask string that is not a hexadecimal number
    /// ([`codes::INVALID_BITFIELD`]).
    UnparsableMask {
        /// The string, as written.
        text: String,
    },
    /// A cell needs more bytes than there are ([`codes::CELL_OUT_OF_BOUNDS`]).
    ShortInput {
        /// Bytes the cell needs.
        needed: usize,
        /// Bytes available.
        len: usize,
    },
    /// An integer raw value outside its storage's range
    /// ([`codes::RAW_OUT_OF_RANGE`]).
    OutOfRange {
        /// The value.
        value: i128,
        /// The storage it was written to.
        storage: Storage,
        /// Smallest storable value.
        min: i128,
        /// Largest storable value.
        max: i128,
    },
    /// An integer raw value that does not fit its bitfield
    /// ([`codes::RAW_OUT_OF_RANGE`]): outside the field's range, or with bits
    /// that the mask does not have.
    BitFieldOutOfRange {
        /// The value.
        value: i128,
        /// The bitfield mask.
        mask: u64,
        /// The bitfield shift.
        shift: u32,
        /// Whether the field is signed.
        signed: bool,
    },
    /// A raw value whose kind does not match the storage
    /// ([`codes::RAW_KIND_MISMATCH`]).
    KindMismatch {
        /// The storage.
        storage: Storage,
        /// The raw value's kind.
        kind: RawKind,
    },
    /// One cell of a whole-table layout decoded or encoded on its own
    /// ([`codes::WHOLE_TABLE_LAYOUT`]).
    WholeTableLayout {
        /// The layout.
        layout: Layout,
    },
    /// A table cell that cannot be encoded ([`Encoding::encode_table`]),
    /// reported with the cell's own error and its code.
    TableCell {
        /// The cell's index in the table (0-based).
        index: usize,
        /// Why the cell cannot be encoded. For a Bosch-subtract table this
        /// is about the derived stored cell, not the table value.
        source: Box<EncodingError>,
    },
    /// An edit refused because a value is a NaN, infinite or subnormal float
    /// and special floats were not explicitly allowed (EC-07,
    /// [`codes::SPECIAL_FLOAT_EDIT`]).
    SpecialFloat {
        /// Whether the refused value is the stored one or the new one.
        operand: EditOperand,
        /// The value's kind, [`RawKind::F32`] or [`RawKind::F64`].
        kind: RawKind,
        /// The value's IEEE 754 bit pattern (see [`Raw::float_bits`]).
        bits: u64,
        /// The value's class.
        class: FloatClass,
    },
}

impl EncodingError {
    /// The registry code this error is reported with.
    #[must_use]
    pub const fn code(&self) -> Code {
        match self {
            Self::UnsupportedStorage { .. } | Self::UnsupportedCombination { .. } => {
                codes::UNSUPPORTED_STORAGE_TYPE
            }
            Self::UnknownEndian { .. } => codes::UNKNOWN_ENDIAN,
            Self::UnknownLayout { .. } => codes::UNKNOWN_LAYOUT,
            Self::InvalidBitField { .. } | Self::UnparsableMask { .. } => codes::INVALID_BITFIELD,
            Self::ShortInput { .. } => codes::CELL_OUT_OF_BOUNDS,
            Self::OutOfRange { .. } | Self::BitFieldOutOfRange { .. } => codes::RAW_OUT_OF_RANGE,
            Self::KindMismatch { .. } => codes::RAW_KIND_MISMATCH,
            Self::WholeTableLayout { .. } => codes::WHOLE_TABLE_LAYOUT,
            Self::SpecialFloat { .. } => codes::SPECIAL_FLOAT_EDIT,
            Self::TableCell { source, .. } => source.code(),
        }
    }

    /// This error as an [`Error`](crate::diag::Severity::Error) diagnostic at
    /// `provenance`, with [`code`](Self::code) and the `Display` message.
    #[must_use]
    pub fn to_diagnostic(&self, provenance: Provenance) -> Diagnostic {
        Diagnostic::error(self.code(), self.to_string(), provenance)
    }
}

impl fmt::Display for EncodingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedStorage { text, problem } => {
                write!(f, "unsupported storage type {text:?}: {problem}")
            }
            Self::UnsupportedCombination { storage, problem } => {
                write!(f, "unsupported encoding for {storage}: {problem}")
            }
            Self::UnknownEndian { text } => {
                write!(f, "unknown endian {text:?}: expected \"big\" or \"little\"")
            }
            Self::UnknownLayout { text } => write!(
                f,
                "unknown table layout {text:?}: expected \"bosch_subtract\" or none"
            ),
            Self::InvalidBitField {
                mask,
                shift,
                problem,
            } => write!(
                f,
                "invalid bitfield (mask {mask:#X}, shift {shift}): {problem}"
            ),
            Self::UnparsableMask { text } => write!(
                f,
                "invalid bitfield mask {text:?}: expected 1 to 16 hexadecimal digits"
            ),
            Self::ShortInput { needed, len } => {
                write!(f, "a {needed}-byte cell does not fit in {len} byte(s)")
            }
            Self::OutOfRange {
                value,
                storage,
                min,
                max,
            } => write!(
                f,
                "value {value} does not fit {storage} (range {min}..={max})"
            ),
            Self::BitFieldOutOfRange {
                value,
                mask,
                shift,
                signed,
            } => write!(
                f,
                "value {value} does not fit the {} bitfield (mask {mask:#X}, shift {shift})",
                if *signed { "signed" } else { "unsigned" }
            ),
            Self::KindMismatch { storage, kind } => {
                write!(f, "{storage} storage cannot hold a raw {kind} value")
            }
            Self::TableCell { index, source } => write!(f, "table cell {index}: {source}"),
            Self::WholeTableLayout { layout } => write!(
                f,
                "a {layout} cell depends on the rest of its table; decode or encode the whole table"
            ),
            Self::SpecialFloat {
                operand,
                kind,
                bits,
                class,
            } => {
                let digits = if matches!(kind, RawKind::F32) { 8 } else { 16 };
                write!(
                    f,
                    "refusing the {operand} {kind} value 0x{bits:0digits$X} ({class}): NaN, \
                     infinite and subnormal floats are edited only when explicitly allowed"
                )
            }
        }
    }
}

impl core::error::Error for EncodingError {}

#[cfg(test)]
mod tests;
