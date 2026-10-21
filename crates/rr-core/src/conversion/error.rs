//! Why a conversion could not be built, or gave no real value.

use core::fmt;

use crate::diag::{Code, Diagnostic, Provenance, codes};
use crate::expr::{ByteSpan, EvalError, ParseError};

/// Which of a conversion's two formulas a problem is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Direction {
    /// The raw-to-real formula (RomRaider `expression`).
    ToReal,
    /// The real-to-raw formula (RomRaider `to_byte`).
    ToRaw,
}

impl Direction {
    /// `to_real` or `to_raw`.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::ToReal => "to_real",
            Self::ToRaw => "to_raw",
        }
    }
}

/// `to_real` or `to_raw`.
impl fmt::Display for Direction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// What is wrong with a lookup table's points.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum LutError {
    /// Fewer than two points: a table needs two to interpolate between.
    TooFewPoints {
        /// How many points it has.
        found: usize,
    },
    /// A raw or real value is NaN or infinite.
    NonFinite {
        /// The point, 0-based.
        index: usize,
    },
    /// A point's raw value is not greater than the one before it, so the
    /// table is not a function of the raw value.
    NotIncreasing {
        /// The point, 0-based (at least 1).
        index: usize,
    },
}

impl fmt::Display for LutError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::TooFewPoints { found } => write!(
                f,
                "the lookup table has {found} point(s); it needs at least 2"
            ),
            Self::NonFinite { index } => write!(
                f,
                "point {} of the lookup table is not a finite number",
                index.saturating_add(1) // no-wrap: a point number in a message
            ),
            Self::NotIncreasing { index } => write!(
                f,
                "the raw value of point {} of the lookup table is not greater than that of \
                 point {}; raw values must strictly increase",
                index.saturating_add(1), // no-wrap: a point number in a message
                index
            ),
        }
    }
}

/// Why [`Conversion::from_spec`](super::Conversion::from_spec) (or one of
/// the form constructors) refused a conversion.
///
/// [`code`](Self::code) is the registry code it is reported with and
/// [`to_diagnostic`](Self::to_diagnostic) makes the
/// [`Error`](crate::diag::Severity::Error) diagnostic. A formula problem
/// carries the byte span of the formula text at fault
/// ([`span`](Self::span)), which the caller maps into its own source.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum ConversionError {
    /// A formula does not parse. Reported with the parse error's own code
    /// (`EXP*`).
    Parse {
        /// Which formula.
        direction: Direction,
        /// Why it does not parse, and where.
        error: ParseError,
    },
    /// A formula refers to a parameter: a conversion takes only `x`.
    ParamRef {
        /// Which formula.
        direction: Direction,
        /// The first reference, as written back (`P8`, `[P9:km/h]`).
        param: String,
        /// Where it is in the formula text.
        span: ByteSpan,
    },
    /// A lookup table's points do not describe a function of the raw value.
    Lut(LutError),
    /// A linear form's factor or offset is NaN or infinite.
    Linear {
        /// The factor.
        m: f64,
        /// The offset.
        b: f64,
    },
}

impl ConversionError {
    /// The registry code this error is reported with.
    #[must_use]
    pub const fn code(&self) -> Code {
        match self {
            Self::Parse { error, .. } => error.code(),
            Self::ParamRef { .. } => codes::CONVERSION_PARAM_REF,
            Self::Lut(_) => codes::INVALID_LUT,
            Self::Linear { .. } => codes::INVALID_LINEAR,
        }
    }

    /// The formula at fault and the byte span in its text, for a formula
    /// problem.
    #[must_use]
    pub const fn span(&self) -> Option<(Direction, ByteSpan)> {
        match self {
            Self::Parse { direction, error } => Some((*direction, error.span())),
            Self::ParamRef {
                direction, span, ..
            } => Some((*direction, *span)),
            Self::Lut(_) | Self::Linear { .. } => None,
        }
    }

    /// This error as an [`Error`](crate::diag::Severity::Error) diagnostic at
    /// `provenance`, with [`code`](Self::code) and the `Display` message.
    #[must_use]
    pub fn to_diagnostic(&self, provenance: Provenance) -> Diagnostic {
        Diagnostic::error(self.code(), self.to_string(), provenance)
    }
}

impl From<LutError> for ConversionError {
    fn from(e: LutError) -> Self {
        Self::Lut(e)
    }
}

impl fmt::Display for ConversionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parse { direction, error } => {
                write!(f, "the {direction} formula does not parse: {error}")
            }
            Self::ParamRef {
                direction, param, ..
            } => write!(
                f,
                "the {direction} formula refers to {param}; a conversion formula may use only \
                 `x`, the value being converted (names are case-sensitive)"
            ),
            Self::Lut(e) => e.fmt(f),
            Self::Linear { m, b } => write!(
                f,
                "a linear conversion needs a finite factor and offset, not m = {m}, b = {b}"
            ),
        }
    }
}

impl core::error::Error for ConversionError {}

/// Why [`Conversion::to_real`](super::Conversion::to_real) gave no value.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum ToRealError {
    /// The result is NaN or infinite; the value is carried, so a caller
    /// that wants to display it as legacy did can.
    NonFinite(f64),
    /// The raw value is not within a lookup table's points (or is NaN). A
    /// table is never extrapolated or clamped.
    OutsideTable {
        /// The raw value.
        raw: f64,
        /// The table's first raw value.
        min: f64,
        /// The table's last raw value.
        max: f64,
    },
    /// The formula could not be evaluated for another reason. Construction
    /// refuses every formula that could give this; it is never expected.
    Eval(EvalError),
}

impl fmt::Display for ToRealError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonFinite(v) => {
                write!(f, "the conversion gives {v}, which is not a finite number")
            }
            Self::OutsideTable { raw, min, max } => write!(
                f,
                "raw value {raw} is outside the lookup table, which covers {min} to {max}"
            ),
            Self::Eval(e) => e.fmt(f),
        }
    }
}

impl core::error::Error for ToRealError {}
