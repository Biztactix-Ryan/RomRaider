//! IEEE 754 float classes and the special-float edit gate (US-RR-19-9,
//! EC-07).
//!
//! The codec decodes and encodes floats bit-exactly: every 4- or 8-byte
//! pattern, NaN payloads (quiet and signalling), `±Inf`, `±0` and subnormals
//! included, comes back as the bytes it was read from. Nothing here changes a
//! value.
//!
//! PROJECT.md EC-07: *"NaN, ±Inf and denormal floats in the ROM: displayed as
//! such, editing is rejected unless explicitly allowed."* So:
//!
//! * [`FloatClass`] says what a float is, computed from its bit pattern
//!   (exponent and fraction fields), never from float arithmetic, so a
//!   signalling NaN is never quieted on the way. [`Raw::float_class`] gives
//!   the class of a decoded value, for display.
//! * The three classes EC-07 names, NaN, infinite and subnormal, are
//!   [`special`](FloatClass::is_special). They are one group with one switch,
//!   [`SpecialFloats`]: EC-07 refuses all three unless explicitly allowed and
//!   gives subnormals no separate rule, so neither does this module.
//! * An edit layer refuses them with [`Raw::check_edit`] or writes through
//!   [`Encoding::edit_into`], which checks both the value stored in the cell
//!   and the value replacing it. A refusal is
//!   [`EncodingError::SpecialFloat`] ([`codes::SPECIAL_FLOAT_EDIT`]).
//!   [`Encoding::encode_into`] stays the unchecked, faithful codec.
//!
//! [`codes::SPECIAL_FLOAT_EDIT`]: crate::diag::codes::SPECIAL_FLOAT_EDIT

use core::fmt;

use super::{Encoding, EncodingError, Raw};

/// The IEEE 754 class of a float value. The sign is not part of the class:
/// it is the top bit of [`Raw::float_bits`] (or `is_sign_negative`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FloatClass {
    /// `+0.0` or `-0.0`: exponent and fraction all zero.
    Zero,
    /// Subnormal (denormal): exponent all zero, fraction non-zero.
    Subnormal,
    /// Normal: exponent neither all zero nor all ones.
    Normal,
    /// `+Inf` or `-Inf`: exponent all ones, fraction zero.
    Infinite,
    /// NaN: exponent all ones, fraction (the payload) non-zero.
    Nan {
        /// `true` for a quiet NaN (top fraction bit set), `false` for a
        /// signalling NaN.
        quiet: bool,
    },
}

impl FloatClass {
    /// The class of the IEEE 754 binary32 value with bit pattern `bits`.
    #[must_use]
    pub const fn of_f32_bits(bits: u32) -> Self {
        classify(bits as u64, 23, 0xFF)
    }

    /// The class of the IEEE 754 binary64 value with bit pattern `bits`.
    #[must_use]
    pub const fn of_f64_bits(bits: u64) -> Self {
        classify(bits, 52, 0x7FF)
    }

    /// `true` for a finite number: zero, subnormal or normal.
    #[must_use]
    pub const fn is_finite(self) -> bool {
        matches!(self, Self::Zero | Self::Subnormal | Self::Normal)
    }

    /// `true` for the classes EC-07 refuses to edit unless explicitly
    /// allowed: NaN, infinite and subnormal.
    #[must_use]
    pub const fn is_special(self) -> bool {
        matches!(self, Self::Nan { .. } | Self::Infinite | Self::Subnormal)
    }

    /// Lower-case name: `zero`, `subnormal`, `normal`, `infinite`,
    /// `quiet NaN` or `signalling NaN`.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Zero => "zero",
            Self::Subnormal => "subnormal",
            Self::Normal => "normal",
            Self::Infinite => "infinite",
            Self::Nan { quiet: true } => "quiet NaN",
            Self::Nan { quiet: false } => "signalling NaN",
        }
    }
}

/// The name, as [`FloatClass::name`].
impl fmt::Display for FloatClass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// The class of an IEEE 754 bit pattern with `fraction_bits` fraction bits
/// and an exponent field whose all-ones value is `exponent_ones`.
const fn classify(bits: u64, fraction_bits: u32, exponent_ones: u64) -> FloatClass {
    let fraction = bits & ((1 << fraction_bits) - 1);
    let exponent = (bits >> fraction_bits) & exponent_ones;
    if exponent == 0 {
        if fraction == 0 {
            FloatClass::Zero
        } else {
            FloatClass::Subnormal
        }
    } else if exponent == exponent_ones {
        if fraction == 0 {
            FloatClass::Infinite
        } else {
            FloatClass::Nan {
                quiet: fraction >> (fraction_bits - 1) == 1,
            }
        }
    } else {
        FloatClass::Normal
    }
}

/// Whether an edit may store, or replace, a [special](FloatClass::is_special)
/// float (NaN, infinite, subnormal). EC-07: refused unless explicitly
/// allowed, so the default is [`Refuse`](Self::Refuse).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum SpecialFloats {
    /// Refuse special floats ([`EncodingError::SpecialFloat`]).
    #[default]
    Refuse,
    /// The user explicitly allowed special floats: edit them like any value.
    Allow,
}

/// Which value of an edit a [special float](FloatClass::is_special) refusal
/// is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EditOperand {
    /// The value currently stored in the cell.
    Stored,
    /// The value the edit would write.
    New,
}

impl EditOperand {
    /// Lower-case name: `stored` or `new`.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Stored => "stored",
            Self::New => "new",
        }
    }
}

/// The name, as [`EditOperand::name`].
impl fmt::Display for EditOperand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

impl Raw {
    /// The IEEE 754 bit pattern of a float value (an `F32`'s 32 bits in the
    /// low half), or `None` for an integer.
    #[must_use]
    pub const fn float_bits(self) -> Option<u64> {
        match self {
            Self::Int(_) => None,
            Self::F32(value) => Some(value.to_bits() as u64),
            Self::F64(value) => Some(value.to_bits()),
        }
    }

    /// The [`FloatClass`] of a float value, from its bits, or `None` for an
    /// integer.
    #[must_use]
    pub const fn float_class(self) -> Option<FloatClass> {
        match self {
            Self::Int(_) => None,
            Self::F32(value) => Some(FloatClass::of_f32_bits(value.to_bits())),
            Self::F64(value) => Some(FloatClass::of_f64_bits(value.to_bits())),
        }
    }

    /// `true` for an integer or a finite float (zero, subnormal, normal);
    /// `false` for NaN and `±Inf`.
    #[must_use]
    pub const fn is_finite_number(self) -> bool {
        match self.float_class() {
            Some(class) => class.is_finite(),
            None => true,
        }
    }

    /// `true` for a [special](FloatClass::is_special) float: NaN, infinite
    /// or subnormal (EC-07). Integers are never special.
    #[must_use]
    pub const fn is_special_float(self) -> bool {
        match self.float_class() {
            Some(class) => class.is_special(),
            None => false,
        }
    }

    /// The EC-07 edit gate for one value of an edit (`operand`: the value in
    /// the cell, or the value replacing it).
    ///
    /// # Errors
    ///
    /// [`EncodingError::SpecialFloat`] if this is a
    /// [special float](Self::is_special_float) and `policy` is
    /// [`SpecialFloats::Refuse`]. Integers and zero or normal floats always
    /// pass.
    pub const fn check_edit(
        self,
        policy: SpecialFloats,
        operand: EditOperand,
    ) -> Result<(), EncodingError> {
        match (self.float_class(), self.float_bits(), policy) {
            (Some(class), Some(bits), SpecialFloats::Refuse) if class.is_special() => {
                Err(EncodingError::SpecialFloat {
                    operand,
                    kind: self.kind(),
                    bits,
                    class,
                })
            }
            _ => Ok(()),
        }
    }
}

impl Encoding {
    /// An edit: [`encode_into`](Self::encode_into) behind the EC-07 gate.
    /// The cell's current value and `raw` are both checked with
    /// [`Raw::check_edit`] before anything is written, so with
    /// [`SpecialFloats::Refuse`] a NaN, infinite or subnormal cell is not
    /// overwritten and none is written.
    ///
    /// # Errors
    ///
    /// * Any error of [`decode`](Self::decode) on the current cell;
    /// * [`EncodingError::SpecialFloat`] for a refused stored value
    ///   ([`EditOperand::Stored`]), then for a refused new value
    ///   ([`EditOperand::New`]);
    /// * any error of [`encode_into`](Self::encode_into).
    ///
    /// On error nothing is written.
    pub fn edit_into(
        self,
        raw: Raw,
        bytes: &mut [u8],
        policy: SpecialFloats,
    ) -> Result<(), EncodingError> {
        self.decode(bytes)?
            .check_edit(policy, EditOperand::Stored)?;
        raw.check_edit(policy, EditOperand::New)?;
        self.encode_into(raw, bytes)
    }
}

#[cfg(test)]
mod tests;
