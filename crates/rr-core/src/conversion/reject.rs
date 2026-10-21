//! Why a real value cannot be written (US-RR-24-11; EC-08, MF-05): the
//! edit-validation taxonomy every refusal of
//! [`to_raw_checked`](super::Conversion::to_raw_checked) belongs to.

use core::fmt;

use crate::diag::{Code, Diagnostic, Provenance, codes};
use crate::encoding::Raw;

use super::range::{RangeError, Undefined};

/// Why [`Conversion::to_raw_checked`](super::Conversion::to_raw_checked)
/// gave no raw value. Nothing was written, wrapped or clamped: a refusal is
/// a value of this type and nothing else (EC-08).
///
/// The four variants are the stable taxonomy an editor branches on:
///
/// | Variant | Meaning | [`code`](Self::code) |
/// |---------|---------|----------------------|
/// | [`OutOfRange`](Self::OutOfRange) | the value is more than half a raw step beyond what the cell can hold | [`EDIT_OUT_OF_RANGE`](codes::EDIT_OUT_OF_RANGE) |
/// | [`NotInvertible`](Self::NotInvertible) | the conversion cannot be solved over the cell's raw range, whatever the value | [`NOT_INVERTIBLE`](codes::NOT_INVERTIBLE), or [`CONVERSION_NO_VALUE`](codes::CONVERSION_NO_VALUE) for [`NotInvertible::NoRange`] |
/// | [`Singular`](Self::Singular) | every raw value has the same real value (a factor of `0`) | [`NOT_INVERTIBLE`](codes::NOT_INVERTIBLE) |
/// | [`NonFinite`](Self::NonFinite) | the value is NaN or infinite, or lies where the conversion has no finite value (past a division by zero, an overflow) | [`EDIT_NOT_FINITE`](codes::EDIT_NOT_FINITE) |
///
/// [`NotInvertible`](Self::NotInvertible) and [`Singular`](Self::Singular)
/// are about the conversion over the raw range ([`Form::inverse`](super::Form::inverse)
/// refuses them before any value is looked at); the other two are about
/// one value.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum Reject {
    /// The value is beyond what the cell can hold: the nearest raw value on
    /// the conversion's raw grid is outside the raw range, so `target` is
    /// more than half a raw step below `min` or above `max` (exactly half
    /// above the highest raw value is refused, below the lowest accepted:
    /// the half-up tie rule). Always `target < min` or `target > max`.
    OutOfRange {
        /// The real value asked for.
        target: f64,
        /// The lowest real value the cell can hold.
        min: f64,
        /// The highest real value the cell can hold.
        max: f64,
        /// The raw value at the end of the range nearest `target`: its real
        /// value is `min` or `max`.
        nearest: Raw,
    },
    /// The conversion cannot be inverted over the raw range, so no value
    /// can be written (MF-05).
    NotInvertible(NotInvertible),
    /// The conversion gives the same real value for every raw value (a
    /// linear factor of `0`, a formula with no `x`), so no raw value is
    /// "the" one for any real value.
    Singular {
        /// The real value every raw value has.
        real: f64,
    },
    /// The value is not finite, or lies where the conversion gives no finite
    /// value.
    NonFinite(NonFinite),
}

/// Why a conversion cannot be inverted ([`Reject::NotInvertible`]).
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum NotInvertible {
    /// The conversion rises over some raw values and falls over others, so
    /// a real value can have more than one raw value (MF-05). A `to_raw`
    /// formula the definition gives is not used instead: which raw value
    /// it picks cannot be checked to be the one.
    NotMonotonic {
        /// A raw value whose real value is above that of the raw value
        /// examined before it.
        rise: Raw,
        /// A raw value whose real value is below that of the raw value
        /// examined before it.
        fall: Raw,
    },
    /// A raw value between two with real values has none (a division by
    /// zero inside the range), so the conversion is not solved across it.
    Gap {
        /// The raw value, and why it has no real value.
        at: Undefined,
    },
    /// No raw value has a real value ([`RangeError`]).
    NoRange(RangeError),
}

/// Why a value is refused as not finite ([`Reject::NonFinite`]).
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum NonFinite {
    /// The value asked for is NaN or infinite.
    Target(f64),
    /// The value is past the last real value the conversion gives on its
    /// side, and the next raw value has no real value (a division by zero,
    /// a square root of a negative number, an overflow), so it is not
    /// "rounded" to `limit`.
    Beyond {
        /// The real value asked for.
        target: f64,
        /// The last real value on that side.
        limit: f64,
        /// The raw value giving it.
        raw: Raw,
        /// The next raw value, and why it has no real value.
        beyond: Undefined,
    },
    /// No raw value near the value has a finite real value of its own (a
    /// linear conversion that overflows there).
    NoValue {
        /// The real value asked for.
        target: f64,
    },
}

impl Reject {
    /// The registry code the refusal is reported with (see the table on
    /// [`Reject`]).
    #[must_use]
    pub const fn code(&self) -> Code {
        match self {
            Self::OutOfRange { .. } => codes::EDIT_OUT_OF_RANGE,
            Self::NotInvertible(NotInvertible::NoRange(e)) => e.code(),
            Self::NotInvertible(_) | Self::Singular { .. } => codes::NOT_INVERTIBLE,
            Self::NonFinite(_) => codes::EDIT_NOT_FINITE,
        }
    }

    /// `true` when the refusal is about the conversion over the raw range,
    /// whatever the value ([`NotInvertible`](Self::NotInvertible),
    /// [`Singular`](Self::Singular)): every value of the cell is refused,
    /// so an editor can disable editing instead of asking for one.
    #[must_use]
    pub const fn is_conversion_wide(&self) -> bool {
        matches!(self, Self::NotInvertible(_) | Self::Singular { .. })
    }

    /// This refusal as an [`Error`](crate::diag::Severity::Error)
    /// diagnostic at `provenance`, with [`code`](Self::code) and the
    /// `Display` message (the reason, for a person).
    #[must_use]
    pub fn to_diagnostic(&self, provenance: Provenance) -> Diagnostic {
        Diagnostic::error(self.code(), self.to_string(), provenance)
    }
}

impl From<NotInvertible> for Reject {
    fn from(e: NotInvertible) -> Self {
        Self::NotInvertible(e)
    }
}

impl From<NonFinite> for Reject {
    fn from(e: NonFinite) -> Self {
        Self::NonFinite(e)
    }
}

impl fmt::Display for Reject {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::OutOfRange {
                target,
                min,
                max,
                nearest,
            } => {
                write!(
                    f,
                    "{target} is outside what the cell can hold, {min} to {max}; the nearest \
                     end is raw value "
                )?;
                write_raw(f, nearest)
            }
            Self::NotInvertible(e) => e.fmt(f),
            Self::Singular { real } => write!(
                f,
                "every raw value converts to {real}, so no raw value can be chosen for another"
            ),
            Self::NonFinite(e) => e.fmt(f),
        }
    }
}

impl fmt::Display for NotInvertible {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::NotMonotonic { rise, fall } => {
                f.write_str(
                    "the conversion is not monotonic over the raw values the cell can hold (it \
                     rises at raw value ",
                )?;
                write_raw(f, rise)?;
                f.write_str(" and falls at raw value ")?;
                write_raw(f, fall)?;
                f.write_str("), so a real value can have more than one raw value to choose from")
            }
            Self::Gap { at } => {
                f.write_str("the conversion has no real value at raw value ")?;
                write_raw(f, at.raw())?;
                write!(
                    f,
                    " ({}), between raw values that have one, so it is not solved across it",
                    at.error()
                )
            }
            Self::NoRange(e) => e.fmt(f),
        }
    }
}

impl fmt::Display for NonFinite {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::Target(v) => write!(f, "{v} is not a finite number"),
            Self::Beyond {
                target,
                limit,
                raw,
                beyond,
            } => {
                write!(
                    f,
                    "{target} is beyond the last real value the conversion gives on that side, \
                     {limit} (raw value "
                )?;
                write_raw(f, raw)?;
                f.write_str("): raw value ")?;
                write_raw(f, beyond.raw())?;
                write!(f, " has none ({})", beyond.error())
            }
            Self::NoValue { target } => write!(
                f,
                "no raw value near {target} has a finite real value, so none can be chosen"
            ),
        }
    }
}

impl core::error::Error for Reject {}

pub(super) fn write_raw(f: &mut fmt::Formatter<'_>, raw: Raw) -> fmt::Result {
    match raw {
        Raw::Int(v) => write!(f, "{v}"),
        Raw::F32(v) => write!(f, "{v}"),
        Raw::F64(v) => write!(f, "{v}"),
    }
}

#[cfg(test)]
mod tests;
