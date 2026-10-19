//! Raw value ranges derived from an [`Encoding`] (US-RR-19-11, MF-15):
//! [`RawRange`], [`IntRange`] and the `raw_range` / `stored_range`
//! helpers. The rules are documented on [`RawRange`].

use super::{BitField, Encoding, EncodingError, Layout, Raw, RawKind, Storage};

/// The integers one stored value can hold: `min ..= max` with the
/// [`holes`](Self::holes) bits clear (see [`RawRange`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct IntRange {
    min: i128,
    max: i128,
    holes: u64,
}

impl IntRange {
    /// The smallest storable value.
    #[must_use]
    pub const fn min(self) -> i128 {
        self.min
    }

    /// The largest storable value.
    #[must_use]
    pub const fn max(self) -> i128 {
        self.max
    }

    /// Bits every storable value has clear in its two's complement form: the
    /// low bits a bitfield shifted by less than its mask's trailing zeros
    /// keeps zero, and the gaps of a non-contiguous unsigned mask. Zero for
    /// a whole storage and for a contiguous field with the default shift,
    /// whose every value in `min..=max` is storable.
    #[must_use]
    pub const fn holes(self) -> u64 {
        self.holes
    }

    /// `true` if `value` is storable: within `min..=max` with every
    /// [`holes`](Self::holes) bit clear.
    #[must_use]
    pub const fn contains(self, value: i128) -> bool {
        // The low 64 bits of the two's complement form: inside the range
        // every value is within 64 bits.
        value >= self.min && value <= self.max && value as u64 & self.holes == 0
    }
}

/// The raw values one stored value can hold, as an integer range or the
/// finite range of a float width.
///
/// The range an editor offers, colours or clamps against comes from the same
/// encoding the codec reads and writes with, so the two cannot disagree: the
/// cell codec checks an integer against exactly this range before writing it
/// ([`Encoding::encode_into`]), and a float range bounds exactly the finite
/// values.
///
/// # Integers
///
/// [`IntRange`] is the set of integers one stored value can hold:
///
/// * no bitfield: the storage's [`int_range`](Storage::int_range), two's
///   complement for the `I*` storages, and `U20` per
///   [`U20_BIT19_SIGN_EXTENDS`](super::U20_BIT19_SIGN_EXTENDS);
/// * a signed bitfield of `w` bits (after the shift):
///   `-2^(w-1) ..= 2^(w-1) - 1`;
/// * an unsigned bitfield: `0 ..= mask >> shift`.
///
/// A field shifted by less than its mask's trailing zeros keeps low zero
/// bits, and an unsigned field's mask may have gaps; those bits are the
/// range's [`holes`](IntRange::holes), which every storable value has clear.
/// [`min`](IntRange::min) and [`max`](IntRange::max) are tight: both are
/// storable, and [`contains`](IntRange::contains) is exactly the codec's
/// check.
///
/// # Floats (MF-15)
///
/// An IEEE 754 float is signed by construction: the sign is a bit of the
/// value, not an attribute of the storage. So a float range is the finite
/// range `-MAX ..= MAX` of its width, whatever "signed" context a definition
/// puts it in, and never `0` or the smallest positive value as a minimum.
///
/// Legacy computed it in `maps/DataCell.java:140-147` from the table's
/// "signed data" flag with the branches swapped (PROJECT.md MF-15): a signed
/// float table got `0.0 ..= Float.MAX_VALUE`, refusing every negative value,
/// and an unsigned one `-Float.MAX_VALUE ..= Float.MAX_VALUE`. Swapping the
/// branches back would be wrong too: legacy derives the flag from the storage
/// type string (`xml/RomAttributeParser.java:73-80`), `float` is always
/// "unsigned" there, and a `0` minimum would refuse the negative values real
/// float tables hold. Here neither context exists for a float
/// ([`Storage::is_signed`] is `false` for both float storages), and both
/// give the same range.
///
/// The `F32` range is in `f32` (`±f32::MAX`), not widened to `f64`, so it
/// compares exactly with an `F32` value. NaN and `±Inf` are outside every
/// float range; subnormals are inside it (they are finite). Whether such a
/// value may be *edited* is the separate EC-07 gate ([`Raw::check_edit`]),
/// which refuses all three unless explicitly allowed.
///
/// # Bosch subtract
///
/// A [`Layout::BoschSubtract`] cell has no value on its own (`value[i] =
/// 2^(8W) - (s[i] + ... + s[N-1])`, see [`Layout::BoschSubtract`]), so a
/// table value has no per-cell range: its bounds depend on its index and the
/// table's length, and which values are storable depends on its neighbours.
/// [`Encoding::raw_range`] refuses it, as the single-cell codec does
/// ([`EncodingError::WholeTableLayout`]). What the codec bounds is each
/// derived stored cell `s[i]`, and [`Encoding::stored_range`] is that range:
/// [`Encoding::encode_table`] refuses a table whose `s[i]` falls outside it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RawRange {
    /// An integer storage, with or without a bitfield.
    Int(IntRange),
    /// [`Storage::F32`]: `-f32::MAX ..= f32::MAX`.
    F32 {
        /// `-f32::MAX`.
        min: f32,
        /// `f32::MAX`.
        max: f32,
    },
    /// [`Storage::F64`]: `-f64::MAX ..= f64::MAX`.
    F64 {
        /// `-f64::MAX`.
        min: f64,
        /// `f64::MAX`.
        max: f64,
    },
}

impl RawRange {
    /// The kind of the values in the range.
    #[must_use]
    pub const fn kind(self) -> RawKind {
        match self {
            Self::Int(_) => RawKind::Int,
            Self::F32 { .. } => RawKind::F32,
            Self::F64 { .. } => RawKind::F64,
        }
    }

    /// The smallest value in the range.
    #[must_use]
    pub const fn min(self) -> Raw {
        match self {
            Self::Int(range) => Raw::Int(range.min),
            Self::F32 { min, .. } => Raw::F32(min),
            Self::F64 { min, .. } => Raw::F64(min),
        }
    }

    /// The largest value in the range.
    #[must_use]
    pub const fn max(self) -> Raw {
        match self {
            Self::Int(range) => Raw::Int(range.max),
            Self::F32 { max, .. } => Raw::F32(max),
            Self::F64 { max, .. } => Raw::F64(max),
        }
    }

    /// The integer range, or `None` for a float range.
    #[must_use]
    pub const fn as_int(self) -> Option<IntRange> {
        match self {
            Self::Int(range) => Some(range),
            Self::F32 { .. } | Self::F64 { .. } => None,
        }
    }

    /// `true` if `raw` is of the range's [`kind`](Self::kind) and in it: an
    /// integer [`IntRange::contains`], a float between `min` and `max`
    /// (so not NaN and not infinite).
    #[must_use]
    pub fn contains(self, raw: Raw) -> bool {
        match (self, raw) {
            (Self::Int(range), Raw::Int(value)) => range.contains(value),
            (Self::F32 { min, max }, Raw::F32(value)) => min <= value && value <= max,
            (Self::F64 { min, max }, Raw::F64(value)) => min <= value && value <= max,
            _ => false,
        }
    }
}

impl Storage {
    /// The raw range of the whole stored value (no bitfield): the
    /// [`int_range`](Self::int_range) of an integer storage, or the finite
    /// range of a float storage (`±MAX`; see [`RawRange`],
    /// MF-15).
    #[must_use]
    pub const fn raw_range(self) -> RawRange {
        match self.int_range() {
            Some((min, max)) => RawRange::Int(IntRange { min, max, holes: 0 }),
            None if matches!(self, Self::F32) => RawRange::F32 {
                min: -f32::MAX,
                max: f32::MAX,
            },
            None => RawRange::F64 {
                min: -f64::MAX,
                max: f64::MAX,
            },
        }
    }
}

impl BitField {
    /// The raw values the field can hold, whatever storage it is in: signed
    /// `-2^(w-1) ..= 2^(w-1) - 1` (`w`: bits after the shift) or unsigned
    /// `0 ..= mask >> shift`, with low bits kept zero by a short shift and an
    /// unsigned mask's gaps as [`IntRange::holes`].
    #[must_use]
    pub const fn raw_range(self) -> IntRange {
        let bits = self.mask >> self.shift;
        if self.signed {
            // A signed mask is contiguous: its only holes are the low bits
            // the shift keeps.
            let low_zeros = (1 << (self.mask.trailing_zeros() - self.shift)) - 1;
            let half = 1_i128 << (self.reach_bits() - self.shift - 1);
            IntRange {
                min: -half,
                max: half - 1 - low_zeros as i128,
                holes: low_zeros,
            }
        } else {
            // Clear bits below the field's top bit.
            let holes = !bits & (u64::MAX >> bits.leading_zeros());
            IntRange {
                min: 0,
                max: bits as i128,
                holes,
            }
        }
    }
}

impl Encoding {
    /// The raw values one cell can hold: [`decode`](Self::decode) gives one
    /// of them for every finite cell, and [`encode_into`](Self::encode_into)
    /// stores an integer exactly when it is in the range. A float range is
    /// `±MAX` of the storage's width, for every "signed" context (MF-15; see
    /// [`RawRange`]).
    ///
    /// # Errors
    ///
    /// [`EncodingError::WholeTableLayout`] for [`Layout::BoschSubtract`],
    /// whose table values have no per-cell range (see [`RawRange`]; the
    /// derived stored cells' range is [`stored_range`](Self::stored_range)).
    pub const fn raw_range(self) -> Result<RawRange, EncodingError> {
        match self.layout {
            Layout::Default => Ok(self.stored_range()),
            Layout::BoschSubtract => Err(EncodingError::WholeTableLayout {
                layout: self.layout,
            }),
        }
    }

    /// The raw values one stored cell can hold, whatever the layout: the
    /// bitfield's [`raw_range`](BitField::raw_range) or the storage's
    /// [`raw_range`](Storage::raw_range). For [`Layout::Default`] this is
    /// [`raw_range`](Self::raw_range); for [`Layout::BoschSubtract`] it
    /// bounds each derived stored cell `s[i]`, which
    /// [`encode_table`](Self::encode_table) checks against it.
    #[must_use]
    pub const fn stored_range(self) -> RawRange {
        match self.bitfield {
            // Encoding::new allows a bitfield on integer storage only.
            Some(field) => RawRange::Int(field.raw_range()),
            None => self.storage.raw_range(),
        }
    }
}

#[cfg(test)]
mod tests;
