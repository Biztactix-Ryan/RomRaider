//! Conversion (concept C4b, US-RR-24): raw values to real values, with a
//! unit and a display format.
//!
//! A [`Conversion`] is one [`Form`] plus the unit and display format the
//! value is shown with:
//!
//! | Form | What it is | Built from |
//! |------|------------|------------|
//! | [`Form::Linear`] | `real = m * x + b` ([`Linear`]) | a factor and an offset |
//! | [`Form::Expr`] | a parsed `to_real` formula and, optionally, its `to_raw` inverse ([`ExprForm`]) | a definition scaling's formula text |
//! | [`Form::Lut`] | a lookup table of `(raw, real)` points, linearly interpolated ([`Lut`]) | a list of points |
//!
//! A definition reader (a RomRaider `<scaling>`, a logger `<conversion>`, a
//! device field) describes a conversion as a [`ConversionSpec`], which
//! names no file format, and [`Conversion::from_spec`] builds it: formulas
//! are parsed once, here, with [`expr::parse`], never per cell or per
//! sample as legacy did (`maps/DataCell.java`, `util/JEPUtil.java`, MF-25).
//!
//! # Fail closed
//!
//! Every form is validated when it is built, and a value of these types is
//! always one that converts: a formula that does not parse keeps its
//! expression code (`EXP*`); a formula naming anything but `x` (a ROM
//! scaling or conversion has no other input) is
//! [`CONVERSION_PARAM_REF`](crate::diag::codes::CONVERSION_PARAM_REF); a
//! table whose raw values do not strictly increase, or a point that is not
//! finite, is [`INVALID_LUT`](crate::diag::codes::INVALID_LUT); a linear
//! form with a non-finite factor or offset is
//! [`INVALID_LINEAR`](crate::diag::codes::INVALID_LINEAR). Each is a
//! [`ConversionError`] that becomes an
//! [`Error`](crate::diag::Severity::Error) diagnostic
//! ([`ConversionError::to_diagnostic`]); nothing falls back to a default
//! scaling.
//!
//! # Raw to real
//!
//! [`Conversion::to_real`] gives the real value of a raw value, or a
//! [`ToRealError`]: a result that is NaN or infinite, or a raw value outside
//! a lookup table's points (a table is never extrapolated). It is a pure
//! function of the conversion and the raw value.
//!
//! * [`Linear`]: `m * x + b`, in `f64`, one multiplication then one
//!   addition (not fused).
//! * [`ExprForm`]: the `to_real` formula with `x` bound to the raw value
//!   ([`Expr::eval`], whose module docs give the JEP numeric rules).
//! * [`Lut`]: the real value of the point with that raw value, or the
//!   straight line between the two points around it. Legacy had no lookup
//!   table conversion (its ROM scalings are formulas only,
//!   `maps/Scale.java`; the logger's `<replace>` maps display text, not
//!   numbers), so these semantics are new: piecewise linear, exact at every
//!   point, and refused outside the first and last point rather than
//!   clamped or extrapolated (PROJECT.md EC-08).
//!
//! # Real to raw, and ranges
//!
//! Writing a real value back is the rest of US-RR-24, built on these types
//! without changing them:
//!
//! * [`Conversion::to_raw_checked`] (US-RR-24-8) inverts a [`Form::Linear`] and an
//!   [`ExprForm`] whose `to_real` is linear (`a*x+b`, divisions, nested
//!   parentheses, recognised symbolically when the form is built:
//!   [`ExprForm::linear`]) exactly: the storable raw value whose real value
//!   is nearest, checked against `to_real` itself, so
//!   `to_raw(to_real(r)) == r`; a value the cell cannot hold is refused
//!   ([`Reject`]). A definition's own `to_raw` is checked, not used
//!   ([`ExprForm::to_raw_mismatch`]);
//! * every other form (a non-linear formula, a lookup table) is solved
//!   numerically (US-RR-24-9) over the encoding's raw range
//!   ([`RawRange`](crate::encoding::RawRange), which the caller passes: a
//!   conversion does not own its encoding, so one conversion serves cells
//!   of any storage), by the same rule, and refused when it is not
//!   monotonic there (MF-05; see [the numeric solve](#the-numeric-solve));
//! * display ranges are [`Conversion::real_range`] (US-RR-24-10): the
//!   minimum and maximum of `to_real` over the raw range, endpoints plus
//!   extrema, so a decreasing scaling reports them the right way round
//!   (MF-13), with the [`Monotonicity`] a solve needs and the lowest raw
//!   value with no real value ([`RealRange::undefined`]);
//! * every refusal is a [`Reject`] (US-RR-24-11): out of range, not
//!   invertible, singular or not finite, each with a reason and a
//!   diagnostic code, never a wrapped or clamped value (EC-08; see
//!   [Edit validation](#edit-validation)).
//!
//! # Linear formulas
//!
//! When an [`ExprForm`] is built, its `to_real` formula's AST is read to
//! decide, symbolically, whether it is `m * x + b` for constants `m` and
//! `b`; if so [`ExprForm::linear`] returns them as a [`Linear`]. The
//! rules, applied bottom-up over the post-order arena:
//!
//! * a number is a constant, and `x` is `1 * x + 0`;
//! * an operator or function whose operands are all constant is a
//!   constant, folded with the evaluator's own arithmetic (so `2^8`,
//!   `pow(2,-8)`, `(5/4)` fold to exactly the value the formula computes);
//! * `-a`, `a + b`, `a - b` of linear (or constant) operands are linear;
//! * `a * c`, `c * a` and `a / c` of a linear `a` and a constant `c` are
//!   linear, except `a / 0` (whose coefficients are not finite);
//! * anything else that involves `x` is not linear: `x*x`, `x/x`, `c/x`,
//!   `x^c` (even `x^1`), `abs(x)`, `x%c`, comparisons, bit operations,
//!   `if(..)`, any function of `x`.
//!
//! Parentheses are not nodes, so `x*(5/4)-40`, `(x-40)/2`, `100-x/2`,
//! `x/256*100` and `14.7*(x/128)` are all recognised. A formula with no `x`
//! is a constant: a [`Linear`] with `m == 0`, which displays but cannot be
//! inverted.
//!
//! ## Precision
//!
//! `m` and `b` are combined in `f64`, in the order the formula's operators
//! give them (`(x-40)/2` is `m = 1/2`, `b = -40/2`). They are therefore the
//! formula's coefficients to within a few ulps, not exactly: a decimal
//! factor such as `0.1` is not an `f64` to begin with, and the parsed AST
//! keeps literals as `f64`, not as decimal text. Exact rationals were
//! considered and rejected: they would make `m` and `b` exact, but not the
//! formula, which is evaluated in `f64` (`to_real` is always the formula
//! itself, never `m * x + b`, because the two can round differently).
//! What makes the inverse exact is that it never trusts `m` and `b` beyond
//! choosing where to look: the inverse ([`Conversion::to_raw_checked`]) checks the
//! raw values around `(real - b) / m` against the formula itself, so `m`
//! and `b` only need to be right to well within one raw step.
//!
//! Each operation of a linear formula is monotonic in `x` in `f64` too
//! (rounding is monotonic), in the direction of the sign of its `m`, as
//! long as no two terms in `x` pull opposite ways (`x*3 - x*2`). The
//! inverse relies on that to know that nothing outside the raw values it
//! checks is nearer; a formula whose terms do pull opposite ways is exact
//! wherever its `f64` arithmetic is (every term below `2^53`).
//!
//! # Real to raw
//!
//! [`Conversion::to_raw_checked`] turns a real value back into the raw value to
//! store: exactly for a [`Form::Linear`] and a [`Form::Expr`] whose
//! `to_real` is linear ([`ExprForm::linear`], US-RR-24-8), by a numeric
//! solve for every other form (US-RR-24-9). Both follow one rule, below.
//! [`Conversion::inverse`] does the per-conversion work (for a solve, the
//! monotonicity check) once, for writing many values; it is a plain value
//! and gives exactly the answers `to_raw_checked` gives.
//!
//! ## The rule
//!
//! The raw value chosen for a real value `t` is the storable raw value
//! whose real value — `to_real`, evaluated exactly as it is for display —
//! is nearest to `t`, distances compared exactly (not in rounded `f64`).
//! When two are equally near, the larger raw value is chosen, which is
//! Java's `Math.round` (half up) on the raw value, as legacy rounded its
//! `to_byte` result (`maps/DataCell.java:447-449`). `t` is refused when
//! the nearest raw value on the conversion's raw grid is not storable: when
//! `t` is more than half a raw step beyond the real value of the lowest or
//! highest raw value (exactly half beyond the highest is refused, beyond
//! the lowest accepted: the same half-up rule), or beyond the last raw
//! value with a finite real value where the conversion overflows (`2*x`
//! over `f64` storage). Nothing is ever wrapped or clamped (EC-08).
//!
//! Distances are between the exact binary values, so at a *decimal*
//! midpoint the nearer binary value wins: with `x*0.1`, `0.25` is raw 2
//! (`0.2000000000000000111` is nearer than `0.3000000000000000444`), where
//! legacy's `Math.round(0.25 / 0.1)` gave 3. Both display 0.05 away; the
//! real value reaches this function as an `f64`, so its decimal text, and
//! any decimal notion of "halfway", is already gone.
//!
//! ## The exact inverse
//!
//! For a linear form, `(t - b) / m` estimates the raw value; the storable values from one
//! below the estimate's floor to two above it are checked against
//! `to_real` itself, and the nearest wins. A linear formula is monotonic in `f64` too (see
//! [Linear formulas](#linear-formulas)), so when the real values of the ends of that window
//! bracket `t`, nothing outside it is nearer. When they do not (the
//! estimate was off by more than a raw step, which `f64` rounding of `m`
//! and `b` cannot do below `2^50` but a formula that cancels badly could),
//! the bracket is found by bisection over the whole raw range instead, so
//! the answer never depends on the estimate. Every step is integer
//! arithmetic on raw-value indices (`i128`): a 4-byte (or 8-byte) storage
//! cannot overflow (MF-05; legacy cast the result to a Java `int`).
//!
//! ## What is guaranteed (linear forms)
//!
//! * `to_real(to_raw(t))` is the real value nearest `t` that the cell can
//!   hold, and `to_raw(to_real(r)) == r` for every storable `r` whose real
//!   value no other storable raw value shares. Two raw values share one
//!   only where `f64` cannot tell them apart (an integer beyond `2^53`, or
//!   a factor below the `f64` resolution of the result); then the larger
//!   of the raw values nearest the estimate is chosen, which displays the
//!   same.
//! * A factor of `0` (a constant) has no inverse: [`Reject::Singular`].
//! * The result is a pure function of the conversion, the raw range and
//!   `t`: no cache, nothing remembered between calls (MF-05: legacy's
//!   `Scale` answered from a per-scale cache).
//!
//! ## The numeric solve
//!
//! Every other form — a formula that is not linear (`x*x`, `sqrt(x)`,
//! `60000/x`, `14.7/(1+x/256)`), a lookup table — is solved over the raw
//! range, in three steps ([`Form::inverse`], then [`Inverse::to_raw_checked`]):
//!
//! 1. **Monotonic, or refused.** [`Form::real_range`] scans the raw range
//!    (every raw value up to [`EXHAUSTIVE_LIMIT`], a sample above). A
//!    conversion that both rises and falls is refused for editing,
//!    [`NotInvertible::NotMonotonic`], naming a raw value where it rises and
//!    one where it falls; so is one with no real value at a raw value
//!    between two that have one ([`NotInvertible::Gap`]: `100/x` over signed
//!    storage), a constant ([`Reject::Singular`]) and one with no value
//!    anywhere ([`NotInvertible::NoRange`]). Each is a
//!    [`NOT_INVERTIBLE`](crate::diag::codes::NOT_INVERTIBLE) diagnostic
//!    (`CONVERSION_NO_VALUE` for the last; [`Reject::to_diagnostic`]).
//!    Raw values with no real value at the ends of the range (`100/x` at 0
//!    over unsigned storage, `sqrt(x-10)` below 10) are not a gap: the
//!    solve runs over the raw values between them, whose ends are found by
//!    bisection where the scan was sampled.
//! 2. **Bisection.** Over that run, on the integer index of the storable
//!    raw values ([`i128`], so no storage, 8-byte included, can overflow:
//!    MF-05, where legacy's search, `maps/Scale.java:79-140`, cast `2^32`
//!    to a Java `int` and stepped in `int`s), for the
//!    first raw value whose real value is at or past `t` in the
//!    conversion's direction. That and the raw value before it are the
//!    bracket; the nearer wins by [the rule](#the-rule), ties to the
//!    larger raw value, and the same half-step rule refuses a value beyond
//!    either end. Past an end where the next raw value has no real value
//!    (the division by zero at `100/x`'s raw 0), the refusal says so:
//!    [`NonFinite::Beyond`].
//! 3. **Check.** The two raw values either side of the answer are
//!    evaluated: each must have a real value, and they must move the
//!    conversion's way. After an exhaustive scan this cannot fail; after a
//!    sampled one it is what makes the answer sound (below).
//!
//! **Plateaus.** Monotonic is not strict: rounding, or a formula like
//! `x - x%4`, gives runs of raw values the same real value. The bracket
//! then decides: a value equal to a plateau's is written as the plateau's
//! lowest raw value (the first at or past it), a value between two
//! plateaus as the raw value of the nearer plateau next to the crossing
//! (the highest of the lower plateau, or the lowest of the higher; for a
//! decreasing conversion, mirrored in value but still lowest-first in raw
//! order). So `to_raw(to_real(r))` displays as `r` does, and is `r` itself
//! whenever no neighbour of `r` shares its real value; across a plateau
//! it can be further than one raw step from `r`, with the same display.
//!
//! **What a sampled scan guarantees** (4- and 8-byte integers, floats).
//! The sample cannot prove monotonicity: a turn, spike or hole narrower
//! than its spacing can be missed. What holds anyway: a turn or hole the
//! sample *saw* is certain, so no monotonic conversion is ever refused for
//! one; the bisection always ends on a real crossing (the value before the
//! bracket is short of `t`, the bracket's own at or past it), and the
//! check makes the answer the nearest among the five raw values around it,
//! refusing (`NotMonotonic`, `Gap`) if those show what the sample missed.
//! The residual risk is a feature away from the answer: a narrow spike or
//! dip elsewhere that comes nearer `t` than the answer is not looked for,
//! so the answer is then locally, not globally, nearest; and a dip below
//! the sampled minimum (or above the maximum) can make a value it reaches
//! be refused as out of range. Neither writes a raw value whose
//! neighbours disagree with it. Every non-linear formula of the pinned
//! definition corpus round-trips over 32-bit storage
//! (`tests/conversion_solve_corpus.rs` in rr-testkit); an exhaustive scan
//! (up to `2^16` raw values) has no residual risk.
//!
//! ## Edit validation
//!
//! [`Conversion::to_raw_checked`] (and [`Inverse::to_raw_checked`]) is the
//! one check a value typed into a cell goes through. It returns the raw
//! value to store, always one the cell's raw range holds, or a [`Reject`]:
//!
//! | [`Reject`] | When | Code |
//! |------------|------|------|
//! | [`OutOfRange`](Reject::OutOfRange) `{ target, min, max, nearest }` | `target` is more than half a raw step below the lowest real value the cell holds or above the highest (the [rule](#the-rule)'s tie: exactly half above the highest is refused, below the lowest accepted); `min`/`max` are the right way round for a decreasing conversion | [`EDIT_OUT_OF_RANGE`](crate::diag::codes::EDIT_OUT_OF_RANGE) |
//! | [`NotInvertible`](Reject::NotInvertible) | the conversion is not monotonic over the raw range, has a hole inside it, or no value at all (MF-05) | [`NOT_INVERTIBLE`](crate::diag::codes::NOT_INVERTIBLE) ([`CONVERSION_NO_VALUE`](crate::diag::codes::CONVERSION_NO_VALUE) for no value) |
//! | [`Singular`](Reject::Singular) | every raw value has one real value (a factor of `0`) | [`NOT_INVERTIBLE`](crate::diag::codes::NOT_INVERTIBLE) |
//! | [`NonFinite`](Reject::NonFinite) | `target` is NaN or infinite, or lies past a raw value where the conversion has no finite value (the pole of `1/(x-10)`, an overflow) | [`EDIT_NOT_FINITE`](crate::diag::codes::EDIT_NOT_FINITE) |
//!
//! [`Reject::to_diagnostic`] gives each its code and a reason for a
//! person. A refusal is final: no code path wraps a raw value around the
//! storage, clamps it to the range or substitutes the nearest end — the
//! raw arithmetic is on `i128` indices that no storage overflows, and the
//! only `clamp` in the module bounds which candidate indices are looked at,
//! never the answer (a source scan in the T6 suite,
//! `tests/t6_edit_reject.rs`, keeps it that way). Legacy narrowed its
//! rounded result with a Java `(int)` cast, which wraps a value past
//! `2^31` (`maps/DataCell.java:447-449`), then clamped it to the storage
//! limits (`:491-498`), so a value typed out of range was silently
//! written as another (EC-08). This module never touches
//! bytes; a caller writes the raw value with
//! [`Encoding::encode_into`](crate::encoding::Encoding::encode_into) only on
//! `Ok`, so a refusal leaves the image as it was (T6, same test).
//!
//! ## A given `to_raw` formula
//!
//! When the definition also gives a `to_raw` (RomRaider `to_byte`, ECUFlash
//! `frexpr`), the inverse of `to_real` (exact, or the numeric solve) is
//! used and the given formula is not. What is written must display as the
//! value typed, and only the inverse of `to_real` guarantees that; legacy
//! trusted the given formula, which definitions in the wild get wrong, and
//! its own check (`maps/Scale.java:64-77`) tried one raw value with a
//! tolerance of 0.001 and loaded the scaling anyway.
//! [`ExprForm::to_raw_mismatch`] finds where a given formula disagrees, so
//! a loader can report it ([`TO_RAW_MISMATCH`](crate::diag::codes::TO_RAW_MISMATCH), a
//! warning).
//!
//! A conversion that cannot be solved (not monotonic, a gap) is refused
//! even when it has a `to_raw`. Legacy wrote whatever the `to_byte`
//! formula gave, rounded, unchecked (`maps/DataCell.java:428-455`); for a
//! conversion that turns, a real value has more than one raw value and
//! nothing can check that the one the formula picks is the one the user
//! means, so it is not trusted to stand in. At the corpus pin, every
//! non-linear ECU `expression` with a `to_byte` is monotonic over unsigned
//! 8-, 16- and 32-bit storage, so this refuses no scaling legacy could
//! edit there. Over signed storage each of them is refused (the
//! reciprocal ones, `K/x` and `14.7/(1+x*k)`, have a pole inside the
//! range; the counts are in `tests/conversion_solve_corpus.rs` in
//! rr-testkit): fail closed, where legacy's unchecked `to_byte` would have
//! written across the pole.
//!
//! # No shared state
//!
//! The module holds no `static`, cache, lock, cell or atomic (`cargo xtask
//! check-globals`, `STATELESS_PATHS` in `xtask/src/globals.rs`). Legacy's
//! `Scale` cached inverse results in a per-scale map whose answers differed
//! from an uncached call (`maps/Scale.java:44`, `:82-85`, `:133-139`,
//! MF-05);
//! here the same input always gives the same output, whatever was converted
//! before.
//!
//! # Example
//!
//! ```
//! use rr_core::conversion::{Conversion, ConversionSpec, Form, FormSpec};
//!
//! let spec = ConversionSpec {
//!     form: FormSpec::Expr { to_real: "x*0.25-50", to_raw: Some("(x+50)/0.25") },
//!     unit: Some("degC"),
//!     format: Some("#.##"),
//! };
//! let c = Conversion::from_spec(&spec).unwrap();
//! assert_eq!(c.to_real(200.0), Ok(0.0));
//! assert_eq!(c.unit(), Some("degC"));
//! assert!(matches!(c.form(), Form::Expr(e) if e.to_raw().is_some()));
//!
//! let bad = ConversionSpec { form: FormSpec::Expr { to_real: "x*P8", to_raw: None }, unit: None, format: None };
//! assert_eq!(Conversion::from_spec(&bad).unwrap_err().code().as_str(), "CNV0001");
//! ```

mod domain;
mod error;
mod inverse;
mod linear;
mod lut;
mod range;
mod reject;

pub use error::{ConversionError, Direction, LutError, ToRealError};
pub use inverse::{Inverse, ToRawMismatch};
pub use lut::Lut;
pub use range::{
    Coverage, EXHAUSTIVE_LIMIT, Extreme, Monotonicity, RangeError, RealRange, Undefined,
};
pub use reject::{NonFinite, NotInvertible, Reject};

use core::fmt;

use crate::encoding::Raw;
use crate::expr::{self, ByteSpan, EvalError, Expr, NodeKind};

/// A raw ↔ real conversion with its unit and display format.
#[derive(Debug, Clone, PartialEq)]
pub struct Conversion {
    form: Form,
    unit: Option<String>,
    format: Option<DisplayFormat>,
}

impl Conversion {
    /// A conversion of `form`, shown in `unit` with `format`.
    #[must_use]
    pub const fn new(form: Form, unit: Option<String>, format: Option<DisplayFormat>) -> Self {
        Self { form, unit, format }
    }

    /// Builds the conversion `spec` describes: parses its formulas, checks
    /// they name only `x`, and validates its table or linear coefficients.
    ///
    /// # Errors
    ///
    /// A [`ConversionError`]; see the [module docs](self#fail-closed).
    pub fn from_spec(spec: &ConversionSpec<'_>) -> Result<Self, ConversionError> {
        let form = Form::from_spec(&spec.form)?;
        Ok(Self::new(
            form,
            spec.unit.map(str::to_owned),
            spec.format.map(DisplayFormat::new),
        ))
    }

    /// How raw values become real ones.
    #[must_use]
    pub const fn form(&self) -> &Form {
        &self.form
    }

    /// The unit real values are in (`degC`, `AFR`), if the definition gives
    /// one.
    #[must_use]
    pub fn unit(&self) -> Option<&str> {
        self.unit.as_deref()
    }

    /// How real values are displayed, if the definition says.
    #[must_use]
    pub const fn format(&self) -> Option<&DisplayFormat> {
        self.format.as_ref()
    }

    /// The real value of the raw value `x`; see [`Form::to_real`].
    ///
    /// # Errors
    ///
    /// As [`Form::to_real`].
    pub fn to_real(&self, x: f64) -> Result<f64, ToRealError> {
        self.form.to_real(x)
    }

    /// The real value of a decoded [`Raw`] value. An integer is converted
    /// to the nearest `f64` (exact up to `2^53`, as legacy's `double` was),
    /// an `f32` exactly.
    ///
    /// # Errors
    ///
    /// As [`Form::to_real`].
    pub fn to_real_raw(&self, raw: Raw) -> Result<f64, ToRealError> {
        self.to_real(raw_to_f64(raw))
    }
}

/// `raw` as the `f64` a conversion takes: an integer rounded to the
/// nearest `f64` (documented on [`Conversion::to_real_raw`]).
fn raw_to_f64(raw: Raw) -> f64 {
    match raw {
        Raw::Int(i) => i as f64, // no-wrap: nearest f64, documented on to_real_raw
        Raw::F32(f) => f64::from(f),
        Raw::F64(f) => f,
    }
}

/// How raw values become real ones: the three forms of C4.
#[derive(Debug, Clone, PartialEq)]
pub enum Form {
    /// `m * x + b`.
    Linear(Linear),
    /// A `to_real` formula and an optional `to_raw` inverse.
    Expr(ExprForm),
    /// A lookup table, linearly interpolated.
    Lut(Lut),
}

impl Form {
    /// Builds the form `spec` describes (see [`Conversion::from_spec`]).
    ///
    /// # Errors
    ///
    /// A [`ConversionError`]; see the [module docs](self#fail-closed).
    pub fn from_spec(spec: &FormSpec<'_>) -> Result<Self, ConversionError> {
        match *spec {
            FormSpec::Linear { m, b } => Ok(Self::Linear(Linear::new(m, b)?)),
            FormSpec::Expr { to_real, to_raw } => Ok(Self::Expr(ExprForm::parse(to_real, to_raw)?)),
            FormSpec::Lut(points) => Ok(Self::Lut(Lut::new(points)?)),
        }
    }

    /// The real value of the raw value `x` (see the
    /// [module docs](self#raw-to-real)).
    ///
    /// # Errors
    ///
    /// [`ToRealError::NonFinite`] when the result is NaN or infinite, and
    /// [`ToRealError::OutsideTable`] when a lookup table has no point at or
    /// on both sides of `x`.
    pub fn to_real(&self, x: f64) -> Result<f64, ToRealError> {
        match self {
            Self::Linear(l) => l.to_real(x),
            Self::Expr(e) => e.to_real(x),
            Self::Lut(t) => t.to_real(x),
        }
    }
}

/// What a definition says about a conversion, independent of the file
/// format it came from. [`Conversion::from_spec`] builds it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ConversionSpec<'a> {
    /// The form, as written.
    pub form: FormSpec<'a>,
    /// The unit, verbatim (RomRaider `units`).
    pub unit: Option<&'a str>,
    /// The display format, verbatim (RomRaider `format`).
    pub format: Option<&'a str>,
}

/// The form of a [`ConversionSpec`], as written.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FormSpec<'a> {
    /// `m * x + b`.
    Linear {
        /// The factor.
        m: f64,
        /// The offset.
        b: f64,
    },
    /// Formula text (RomRaider `expression` / `to_byte`, ECUFlash `toexpr`
    /// / `frexpr`, logger `expr`).
    Expr {
        /// Raw (`x`) to real.
        to_real: &'a str,
        /// Real (`x`) to raw, if the definition gives one.
        to_raw: Option<&'a str>,
    },
    /// `(raw, real)` points, raw values strictly increasing.
    Lut(&'a [(f64, f64)]),
}

/// A linear conversion: `real = m * x + b`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Linear {
    m: f64,
    b: f64,
}

impl Linear {
    /// `m * x + b`. A factor of zero is allowed (a constant: it displays,
    /// but cannot be inverted).
    ///
    /// # Errors
    ///
    /// [`ConversionError::Linear`] when `m` or `b` is NaN or infinite.
    pub fn new(m: f64, b: f64) -> Result<Self, ConversionError> {
        if m.is_finite() && b.is_finite() {
            Ok(Self { m, b })
        } else {
            Err(ConversionError::Linear { m, b })
        }
    }

    /// The factor.
    #[must_use]
    pub const fn m(self) -> f64 {
        self.m
    }

    /// The offset.
    #[must_use]
    pub const fn b(self) -> f64 {
        self.b
    }

    /// `m * x + b`.
    ///
    /// # Errors
    ///
    /// [`ToRealError::NonFinite`] when the result is NaN or infinite.
    pub fn to_real(self, x: f64) -> Result<f64, ToRealError> {
        finite(self.m * x + self.b)
    }
}

/// A formula conversion: `to_real`, and the `to_raw` inverse if the
/// definition gives one. Both use only `x`.
#[derive(Debug, Clone, PartialEq)]
pub struct ExprForm {
    to_real: Expr,
    to_raw: Option<Expr>,
    /// `to_real`'s coefficients if it is linear ([`linear::detect`]):
    /// derived from `to_real` once, here, so equal formulas have equal
    /// values.
    linear: Option<Linear>,
}

impl ExprForm {
    /// From parsed formulas.
    ///
    /// # Errors
    ///
    /// [`ConversionError::ParamRef`] when either formula refers to anything
    /// but `x`.
    pub fn new(to_real: Expr, to_raw: Option<Expr>) -> Result<Self, ConversionError> {
        only_x(&to_real, Direction::ToReal)?;
        if let Some(inverse) = &to_raw {
            only_x(inverse, Direction::ToRaw)?;
        }
        let linear = linear::detect(&to_real);
        Ok(Self {
            to_real,
            to_raw,
            linear,
        })
    }

    /// Parses `to_real` and `to_raw` within the default
    /// [`Limits`](crate::expr::Limits) and builds the form.
    ///
    /// # Errors
    ///
    /// [`ConversionError::Parse`] when a formula does not parse, and as
    /// [`new`](Self::new).
    pub fn parse(to_real: &str, to_raw: Option<&str>) -> Result<Self, ConversionError> {
        let forward = parse(to_real, Direction::ToReal)?;
        let inverse = to_raw.map(|t| parse(t, Direction::ToRaw)).transpose()?;
        Self::new(forward, inverse)
    }

    /// Raw (`x`) to real.
    #[must_use]
    pub const fn to_real_expr(&self) -> &Expr {
        &self.to_real
    }

    /// Real (`x`) to raw, if the definition gives one.
    #[must_use]
    pub const fn to_raw(&self) -> Option<&Expr> {
        self.to_raw.as_ref()
    }

    /// The `to_real` formula at `x`.
    ///
    /// # Errors
    ///
    /// [`ToRealError::NonFinite`] when the result is NaN or infinite.
    pub fn to_real(&self, x: f64) -> Result<f64, ToRealError> {
        match self.to_real.eval(x, &[]) {
            Ok(v) => Ok(v),
            Err(EvalError::NonFinite(v)) => Err(ToRealError::NonFinite(v)),
            // `new` refused every formula with a parameter reference, so
            // the zero values given always match; kept as an error, never a
            // panic.
            Err(other) => Err(ToRealError::Eval(other)),
        }
    }
}

fn parse(text: &str, direction: Direction) -> Result<Expr, ConversionError> {
    expr::parse(text).map_err(|error| ConversionError::Parse { direction, error })
}

/// Refuses a formula with a parameter reference, pointing at the first.
fn only_x(formula: &Expr, direction: Direction) -> Result<(), ConversionError> {
    let Some(param) = formula.params().first() else {
        return Ok(());
    };
    // Leaves are in text order in the post-order arena, so the first
    // `Param` node is the first reference, which is `params()[0]`.
    let span = formula
        .nodes()
        .iter()
        .find(|n| matches!(n.kind(), NodeKind::Param(_)))
        .map_or(ByteSpan::new(0, 0), expr::Node::span);
    Err(ConversionError::ParamRef {
        direction,
        param: param.to_string(),
        span,
    })
}

/// `v`, or [`ToRealError::NonFinite`] when it is NaN or infinite.
fn finite(v: f64) -> Result<f64, ToRealError> {
    if v.is_finite() {
        Ok(v)
    } else {
        Err(ToRealError::NonFinite(v))
    }
}

/// How real values are displayed: the definition's format pattern,
/// verbatim.
///
/// RomRaider writes these as `java.text.DecimalFormat` patterns (`#.##`,
/// `0.000`, `%`; legacy defaulted a missing one to `#.##`,
/// `maps/Scale.java:38`). The pattern is kept as written; turning it into
/// text is the UI's job, and a missing format stays missing here rather
/// than taking a default (D-012).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DisplayFormat(String);

impl DisplayFormat {
    /// The format `pattern`.
    #[must_use]
    pub fn new(pattern: &str) -> Self {
        Self(pattern.to_owned())
    }

    /// The pattern, as written.
    #[must_use]
    pub fn pattern(&self) -> &str {
        &self.0
    }
}

/// The pattern, as written.
impl fmt::Display for DisplayFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests;
