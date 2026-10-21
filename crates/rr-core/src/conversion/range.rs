//! Display ranges (US-RR-24-10, MF-13): the smallest and largest real value
//! a conversion gives over the raw values a cell can hold.
//!
//! Legacy took a table's maximum real value as `f(maxBin)` and its minimum
//! as `f(minBin)` (`maps/Table.java:444-450`), which holds only for an
//! increasing scaling: for a decreasing one (`-0.5*x+100`, `100-x/2`) the
//! two come out swapped, and for one that turns (`(x-128)^2`) both can be
//! wrong (PROJECT.md MF-13). Here the range is the minimum and maximum of
//! the conversion over the raw values themselves, looked for at the
//! endpoints and wherever the function can turn, so the direction of the
//! scaling never matters. See [`Form::real_range`] for how each form is
//! searched and what the result guarantees.

use core::fmt;

use crate::diag::{Code, Diagnostic, Provenance, codes};
use crate::encoding::{Raw, RawRange};

use super::domain::{Domain, first_true};
use super::{Conversion, Form, Linear, Lut, ToRealError};

/// The largest number of raw values a formula ([`Form::Expr`]) is
/// evaluated at one by one; a larger raw range is sampled (see
/// [`Form::real_range`]). `2^16`: every 8- and 16-bit storage and every
/// bitfield of up to 16 bits is exact.
pub const EXHAUSTIVE_LIMIT: u128 = 1 << 16;

/// Intervals of the even grid a sampled scan lays over the raw range.
const GRID: i128 = 1 << 14;

/// More ternary-search rounds than any bracket needs (see [`refine`]).
const TERNARY_ROUNDS: u32 = 160;

/// Sampled local minima (and, separately, maxima) refined, most extreme
/// first.
const REFINE: usize = 8;

impl Conversion {
    /// The display range over `raw`: see [`Form::real_range`].
    ///
    /// # Errors
    ///
    /// As [`Form::real_range`].
    pub fn real_range(&self, raw: RawRange) -> Result<RealRange, RangeError> {
        self.form.real_range(raw)
    }
}

impl Form {
    /// The smallest and largest real value the conversion gives over the
    /// raw values in `raw` (the cell encoding's
    /// [`raw_range`](crate::encoding::Encoding::raw_range): a conversion
    /// does not own its encoding), with where each is attained, whether the
    /// conversion rises or falls over the range, and the lowest raw value
    /// found to have no real value.
    ///
    /// Only storable raw values count: an integer range's
    /// [`holes`](crate::encoding::IntRange::holes) are skipped, and a float
    /// range is its finite values. Raw values with no real value (a
    /// division by zero, a NaN, a raw value outside a lookup table) are left
    /// out of the minimum, maximum and [`Monotonicity`], and the lowest one
    /// found is [`RealRange::undefined`]: an editor shows the range of the
    /// values that exist, and an edit check can refuse a conversion with a
    /// hole in it rather than solve across one.
    ///
    /// # How each form is searched
    ///
    /// * [`Form::Linear`] — exact. `m * x + b` is monotonic in `x` (also as
    ///   rounded `f64`), so its extremes are at the ends of the raw range,
    ///   or, where it overflows there, at the last raw values whose result
    ///   is finite, found by bisection from `x = 0` (where it is `b`).
    /// * [`Form::Lut`] — exact. Between two table points the conversion is a
    ///   straight line, so over the storable raw values its extremes, turns
    ///   and holes are at the ends of the raw range, at the storable value
    ///   at or just below each table point and at the first one above it.
    ///   One caveat: the interpolation rounds, and a value next to a point
    ///   may differ from the point's own value by an ulp in the "wrong"
    ///   direction; such sub-ulp turns are not looked for. A segment that
    ///   overflows (`f64::MAX` apart) makes the result
    ///   [`Coverage::Sampled`].
    /// * [`Form::Expr`] over at most [`EXHAUSTIVE_LIMIT`] raw values —
    ///   exact: the formula is evaluated at every storable raw value.
    /// * [`Form::Expr`] over more (32- and 64-bit integers, floats) —
    ///   [`Coverage::Sampled`]. The formula is evaluated on an even grid of
    ///   `2^14 + 1` points across the raw range (endpoints included; for a
    ///   float range the grid is even in IEEE 754 order, so geometric in
    ///   magnitude), and, for an integer range, at `0`, `±2^k` and
    ///   `±1.5 * 2^k`. The eight lowest sampled local minima and the eight
    ///   highest sampled local maxima (a sample next to one with no value
    ///   counts as both) are each refined by a ternary search between its
    ///   two neighbouring samples, and every value evaluated anywhere counts.
    ///
    ///   What a sampled result guarantees: [`min`](RealRange::min) and
    ///   [`max`](RealRange::max) are values the formula really gives, at the
    ///   raw values reported, so the true range contains them; a
    ///   [`Monotonicity::NonMonotonic`] verdict and an
    ///   [`undefined`](RealRange::undefined) raw value are certain (each was
    ///   seen). What it does not: an extremum, turn or hole narrower than
    ///   the sample spacing, away from the samples' own extremes, can be
    ///   missed. The bound is exact whenever the formula is monotonic, and
    ///   whenever it is unimodal between the samples around each extremum.
    ///
    /// The search is a pure function of the conversion and the raw range:
    /// no cache, no randomness, the same result whatever was asked before.
    ///
    /// # Errors
    ///
    /// [`RangeError::NoValue`] when no raw value examined has a real value.
    pub fn real_range(&self, raw: RawRange) -> Result<RealRange, RangeError> {
        let domain = Domain::new(raw);
        match self {
            Self::Linear(l) => Ok(linear(*l, domain)),
            Self::Lut(t) => lut(t, domain),
            Self::Expr(e) if domain.len() <= EXHAUSTIVE_LIMIT => {
                exhaustive(domain, &|x| e.to_real(x))
            }
            Self::Expr(e) => sampled(domain, &|x| e.to_real(x)),
        }
    }
}

/// The display range of a conversion over a raw range: see
/// [`Form::real_range`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RealRange {
    min: Extreme,
    max: Extreme,
    monotonicity: Monotonicity,
    coverage: Coverage,
    undefined: Option<Undefined>,
    shape: Shape,
}

/// Where a scan found real values, and where it saw the conversion turn:
/// what a numeric solve (`super::inverse`) needs beyond the range itself.
/// Every raw value here is one the scan examined.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Shape {
    /// The lowest raw value with a real value.
    pub(super) first: Raw,
    /// The highest raw value with a real value.
    pub(super) last: Raw,
    /// The highest raw value below `first` with no real value.
    pub(super) before: Option<Raw>,
    /// The lowest raw value above `last` with no real value.
    pub(super) after: Option<Raw>,
    /// The lowest raw value with no real value between two that have one.
    pub(super) gap: Option<Undefined>,
    /// The first raw value whose real value is above the one examined
    /// before it.
    pub(super) rise: Option<Raw>,
    /// The first raw value whose real value is below the one examined
    /// before it.
    pub(super) fall: Option<Raw>,
}

impl RealRange {
    /// The smallest real value, and a raw value giving it.
    #[must_use]
    pub const fn min(&self) -> Extreme {
        self.min
    }

    /// The largest real value, and a raw value giving it.
    #[must_use]
    pub const fn max(&self) -> Extreme {
        self.max
    }

    /// Whether the conversion rises or falls over the raw values that have
    /// a real value.
    #[must_use]
    pub const fn monotonicity(&self) -> Monotonicity {
        self.monotonicity
    }

    /// Whether every raw value was accounted for.
    #[must_use]
    pub const fn coverage(&self) -> Coverage {
        self.coverage
    }

    /// The lowest raw value found with no real value, and why; `None` if
    /// none was found (with [`Coverage::Exact`]: none exists).
    #[must_use]
    pub const fn undefined(&self) -> Option<Undefined> {
        self.undefined
    }

    /// Where the real values are and where the conversion turns.
    pub(super) const fn shape(&self) -> Shape {
        self.shape
    }
}

/// An extreme real value and a raw value where the conversion gives it:
/// for an exact range of a [`Form::Linear`], the end of the raw range;
/// otherwise the lowest raw value examined that gives it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Extreme {
    real: f64,
    raw: Raw,
}

impl Extreme {
    /// The real value.
    #[must_use]
    pub const fn real(self) -> f64 {
        self.real
    }

    /// A raw value giving it.
    #[must_use]
    pub const fn raw(self) -> Raw {
        self.raw
    }
}

/// A raw value with no real value, and why.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Undefined {
    raw: Raw,
    error: ToRealError,
}

impl Undefined {
    /// `raw`, which has no real value because of `error`.
    pub(super) const fn new(raw: Raw, error: ToRealError) -> Self {
        Self { raw, error }
    }

    /// The raw value.
    #[must_use]
    pub const fn raw(self) -> Raw {
        self.raw
    }

    /// Why it has no real value.
    #[must_use]
    pub const fn error(self) -> ToRealError {
        self.error
    }
}

/// How a conversion's real value moves as the raw value increases, over
/// the raw values that have one. Not strict: rounding can give neighbouring
/// raw values the same real value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Monotonicity {
    /// Every raw value gives the same real value.
    Constant,
    /// Never falls, and rises somewhere.
    Increasing,
    /// Never rises, and falls somewhere.
    Decreasing,
    /// Both rises and falls. A step across a raw value with no real value
    /// counts like any other (`1/(x-10)` falls, then jumps up across 10).
    NonMonotonic,
}

/// How completely a range was searched (see [`Form::real_range`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Coverage {
    /// Every storable raw value is accounted for, by evaluation or by the
    /// shape of the form.
    Exact,
    /// A sample of the raw values was evaluated: the extremes are real
    /// values the conversion gives, but a narrow feature between samples
    /// can be missed.
    Sampled,
}

/// Why [`Form::real_range`] has no range.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum RangeError {
    /// No raw value examined has a real value: the conversion is NaN,
    /// infinite or outside its table over the whole raw range (as far as
    /// `coverage` says it was searched).
    NoValue {
        /// The lowest raw value, and why it has no real value.
        lowest: Undefined,
        /// How completely the raw range was searched.
        coverage: Coverage,
    },
}

impl RangeError {
    /// The registry code this error is reported with.
    #[must_use]
    pub const fn code(&self) -> Code {
        match self {
            Self::NoValue { .. } => codes::CONVERSION_NO_VALUE,
        }
    }

    /// This error as an [`Error`](crate::diag::Severity::Error) diagnostic at
    /// `provenance`, with [`code`](Self::code) and the `Display` message.
    #[must_use]
    pub fn to_diagnostic(&self, provenance: Provenance) -> Diagnostic {
        Diagnostic::error(self.code(), self.to_string(), provenance)
    }
}

impl fmt::Display for RangeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoValue { lowest, .. } => {
                f.write_str("no raw value the cell can hold has a real value (raw value ")?;
                match lowest.raw {
                    Raw::Int(v) => write!(f, "{v}"),
                    Raw::F32(v) => write!(f, "{v}"),
                    Raw::F64(v) => write!(f, "{v}"),
                }?;
                write!(f, ": {})", lowest.error)
            }
        }
    }
}

impl core::error::Error for RangeError {}

/// `m * x + b` over the domain: exact, from the ends of its finite part.
fn linear(l: Linear, d: Domain) -> RealRange {
    // The same arithmetic as `Linear::to_real`.
    let f = |i: i128| l.m() * d.value(i) + l.b();
    let finite = |i: i128| f(i).is_finite();
    // Every raw range holds 0, where the result is `b`: finite. The result
    // is monotonic, so it is finite on one run of indices around 0.
    let zero = d.partition(|i| d.value(i) >= 0.0);
    let first = first_true(d.lo(), zero, finite);
    let last = first_true(zero, d.hi() + 1, |i| !finite(i)) - 1;
    let lowest = Extreme {
        real: f(first),
        raw: d.raw(first),
    };
    let highest = Extreme {
        real: f(last),
        raw: d.raw(last),
    };
    let (min, max, monotonicity) = if lowest.real < highest.real {
        (lowest, highest, Monotonicity::Increasing)
    } else if lowest.real > highest.real {
        (highest, lowest, Monotonicity::Decreasing)
    } else {
        (lowest, lowest, Monotonicity::Constant)
    };
    let undefined = if first != d.lo() {
        Some(d.lo())
    } else if last != d.hi() {
        Some(last + 1)
    } else {
        None
    }
    .map(|i| Undefined {
        raw: d.raw(i),
        error: ToRealError::NonFinite(f(i)),
    });
    // Examined: the ends of the finite run (one run of indices), and the
    // raw values just outside it.
    let shape = Shape {
        first: d.raw(first),
        last: d.raw(last),
        before: (first != d.lo()).then(|| d.raw(first - 1)),
        after: (last != d.hi()).then(|| d.raw(last + 1)),
        gap: None,
        rise: (monotonicity == Monotonicity::Increasing).then(|| d.raw(last)),
        fall: (monotonicity == Monotonicity::Decreasing).then(|| d.raw(last)),
    };
    RealRange {
        min,
        max,
        monotonicity,
        coverage: Coverage::Exact,
        undefined,
        shape,
    }
}

/// A lookup table over the domain: the ends of the domain and the storable
/// values around every table point (see [`Form::real_range`]).
fn lut(t: &Lut, d: Domain) -> Result<RealRange, RangeError> {
    // The lowest index is scanned first anyway.
    let mut indices = vec![d.hi()];
    for &(raw, _) in t.points() {
        let above = d.partition(|i| d.value(i) > raw);
        indices.extend([above - 1, above]);
    }
    indices.retain(|i| (d.lo()..=d.hi()).contains(i));
    indices.sort_unstable();
    let overflow = indices
        .iter()
        .any(|&i| matches!(t.to_real(d.value(i)), Err(ToRealError::NonFinite(_))));
    let coverage = if overflow {
        Coverage::Sampled
    } else {
        Coverage::Exact
    };
    scan(d, &|x| t.to_real(x), indices, coverage)
}

/// Every storable raw value, in order. Only for a domain of at most
/// [`EXHAUSTIVE_LIMIT`] values: [`Form::real_range`] samples a larger one.
fn exhaustive(d: Domain, f: &Eval<'_>) -> Result<RealRange, RangeError> {
    // A wrong dispatch in `real_range` would evaluate a 32-bit range one by
    // one (2^32 calls, minutes) and give the right answer slowly; this makes
    // it fail at once in a test build instead (T7: such a mutant is caught,
    // not a TIMEOUT). Counted from the ends, not with `Domain::len`, so a
    // wrong `len` is caught here too.
    debug_assert!(
        d.hi() - d.lo() < 1 << 16,
        "exhaustive scan of raw indices {}..={} (limit {EXHAUSTIVE_LIMIT} values)",
        d.lo(),
        d.hi()
    );
    scan(d, f, d.lo()..=d.hi(), Coverage::Exact)
}

/// A grid and landmark sample, refined around its extremes (see
/// [`Form::real_range`]).
fn sampled(d: Domain, f: &Eval<'_>) -> Result<RealRange, RangeError> {
    let mut indices = sample(d);
    let samples: Vec<(i128, Result<f64, ToRealError>)> =
        indices.iter().map(|&i| (i, f(d.value(i)))).collect();
    for (lo, hi, sense) in candidates(&samples) {
        refine(d, f, lo, hi, sense, &mut indices);
    }
    indices.sort_unstable();
    scan(d, f, indices, Coverage::Sampled)
}

/// The sample of a sampled scan, increasing: the even grid and, for an
/// integer range, the storable values at and around `0`, `±2^k` and
/// `±1.5 * 2^k`.
fn sample(d: Domain) -> Vec<i128> {
    let span = d.hi() - d.lo();
    let mut indices: Vec<i128> = (0..=GRID).map(|k| d.lo() + span * k / GRID).collect();
    if d.is_int() {
        let mut magnitude = 1.0_f64;
        for _ in 0..64 {
            for v in [
                magnitude,
                1.5 * magnitude,
                -magnitude,
                -1.5 * magnitude,
                0.0,
            ] {
                indices.extend(d.floor(v));
                indices.extend(d.ceil(v));
            }
            magnitude *= 2.0;
        }
    }
    indices.sort_unstable();
    indices.dedup();
    indices
}

type Eval<'a> = dyn Fn(f64) -> Result<f64, ToRealError> + 'a;

/// Which extreme a refinement looks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Sense {
    Min,
    Max,
}

/// The brackets to refine: the [`REFINE`] lowest sampled local minima and
/// the [`REFINE`] highest local maxima, each between its neighbouring
/// samples. A sample with no value is no neighbour, so one beside it is
/// both a minimum and a maximum.
fn candidates(samples: &[(i128, Result<f64, ToRealError>)]) -> Vec<(i128, i128, Sense)> {
    let mut minima = Vec::new();
    let mut maxima = Vec::new();
    for (j, &(i, result)) in samples.iter().enumerate() {
        let Ok(v) = result else { continue };
        let left = j.checked_sub(1).and_then(|k| samples.get(k));
        let right = samples.get(j + 1);
        let lo = left.map_or(i, |p| p.0);
        let hi = right.map_or(i, |p| p.0);
        let neighbours = [left, right].map(|p| p.and_then(|p| p.1.ok()));
        if neighbours.iter().flatten().all(|&n| v <= n) {
            minima.push((v, lo, hi));
        }
        if neighbours.iter().flatten().all(|&n| v >= n) {
            maxima.push((v, lo, hi));
        }
    }
    // Most extreme first; equal values in raw order (the sort is stable).
    minima.sort_by(|a, b| a.0.total_cmp(&b.0));
    maxima.sort_by(|a, b| b.0.total_cmp(&a.0));
    let minima = minima
        .into_iter()
        .take(REFINE)
        .map(|(_, lo, hi)| (lo, hi, Sense::Min));
    let maxima = maxima
        .into_iter()
        .take(REFINE)
        .map(|(_, lo, hi)| (lo, hi, Sense::Max));
    minima.chain(maxima).collect()
}

/// A ternary search for the `sense` extreme over indices `lo ..= hi`,
/// adding every index it evaluates to `seen`. Exact for a function
/// unimodal over the bracket; a raw value with no real value counts as the
/// worst possible.
fn refine(d: Domain, f: &Eval<'_>, mut lo: i128, mut hi: i128, sense: Sense, seen: &mut Vec<i128>) {
    let score = |i: i128| match (f(d.value(i)), sense) {
        (Ok(v), Sense::Max) => v,
        (Ok(v), Sense::Min) => -v,
        (Err(_), _) => f64::NEG_INFINITY,
    };
    // Each round keeps at most about two thirds of the bracket, and no
    // bracket is wider than 2^66 indices, so fewer than `TERNARY_ROUNDS`
    // rounds leave three; the bound only keeps a broken score from looping.
    // On a tie (a plateau, or no value at either point) the bracket keeps
    // its lower part.
    for _ in 0..TERNARY_ROUNDS {
        let third = (hi - lo) / 3;
        if third == 0 {
            break;
        }
        let (m1, m2) = (lo + third, hi - third);
        seen.extend([m1, m2]);
        if score(m1) >= score(m2) {
            hi = m2;
        } else {
            lo = m1;
        }
    }
    // At most three indices are left.
    seen.extend([lo, lo + (hi - lo) / 2, hi]);
}

/// Evaluates `f` at the domain's lowest index and then at `indices`, which
/// must be increasing (repeats are harmless), and gathers the range.
fn scan(
    d: Domain,
    f: &Eval<'_>,
    indices: impl IntoIterator<Item = i128>,
    coverage: Coverage,
) -> Result<RealRange, RangeError> {
    let point = |i: i128| (d.raw(i), f(d.value(i)));
    let mut scan = Scan::start(point(d.lo()));
    for i in indices {
        scan.push(point(i));
    }
    scan.finish(coverage)
}

/// A running range over points in increasing raw order.
#[expect(
    clippy::large_enum_variant,
    reason = "one per scan, on the stack, moved once: boxing would only add an allocation"
)]
enum Scan {
    /// Nothing with a real value yet: the lowest point, and the latest.
    NoValue { lowest: Undefined, latest: Raw },
    /// At least one real value.
    Values {
        min: Extreme,
        max: Extreme,
        last: f64,
        undefined: Option<Undefined>,
        /// The first point with no value since the last with one.
        pending: Option<Undefined>,
        shape: Shape,
    },
}

impl Scan {
    fn start((raw, result): (Raw, Result<f64, ToRealError>)) -> Self {
        match result {
            Ok(real) => Self::first(Extreme { real, raw }, None, None),
            Err(error) => Self::NoValue {
                lowest: Undefined { raw, error },
                latest: raw,
            },
        }
    }

    fn first(at: Extreme, undefined: Option<Undefined>, before: Option<Raw>) -> Self {
        Self::Values {
            min: at,
            max: at,
            last: at.real,
            undefined,
            pending: None,
            shape: Shape {
                first: at.raw,
                last: at.raw,
                before,
                after: None,
                gap: None,
                rise: None,
                fall: None,
            },
        }
    }

    fn push(&mut self, (raw, result): (Raw, Result<f64, ToRealError>)) {
        match (&mut *self, result) {
            (Self::NoValue { lowest, latest }, Ok(real)) => {
                *self = Self::first(Extreme { real, raw }, Some(*lowest), Some(*latest));
            }
            (Self::NoValue { latest, .. }, Err(_)) => *latest = raw,
            (
                Self::Values {
                    min,
                    max,
                    last,
                    pending,
                    shape,
                    ..
                },
                Ok(real),
            ) => {
                if real < min.real {
                    *min = Extreme { real, raw };
                }
                if real > max.real {
                    *max = Extreme { real, raw };
                }
                if real > *last && shape.rise.is_none() {
                    shape.rise = Some(raw);
                }
                if real < *last && shape.fall.is_none() {
                    shape.fall = Some(raw);
                }
                if shape.gap.is_none() {
                    shape.gap = *pending;
                }
                *pending = None;
                *last = real;
                shape.last = raw;
            }
            (
                Self::Values {
                    undefined, pending, ..
                },
                Err(error),
            ) => {
                let here = Undefined { raw, error };
                undefined.get_or_insert(here);
                pending.get_or_insert(here);
            }
        }
    }

    fn finish(self, coverage: Coverage) -> Result<RealRange, RangeError> {
        match self {
            Self::NoValue { lowest, .. } => Err(RangeError::NoValue { lowest, coverage }),
            Self::Values {
                min,
                max,
                undefined,
                pending,
                mut shape,
                ..
            } => {
                shape.after = pending.map(Undefined::raw);
                Ok(RealRange {
                    min,
                    max,
                    monotonicity: match (shape.rise, shape.fall) {
                        (None, None) => Monotonicity::Constant,
                        (Some(_), None) => Monotonicity::Increasing,
                        (None, Some(_)) => Monotonicity::Decreasing,
                        (Some(_), Some(_)) => Monotonicity::NonMonotonic,
                    },
                    coverage,
                    undefined,
                    shape,
                })
            }
        }
    }
}

#[cfg(test)]
mod tests;
