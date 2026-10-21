//! Real to raw: the exact inverse of a linear conversion (US-RR-24-8), the
//! bounded numeric solve of any other (US-RR-24-9; MF-05) and the edit
//! check that refuses a value with a reason, never wrapping or clamping it
//! (US-RR-24-11; EC-08). The rule, how each is found and what each
//! guarantees are in the [conversion module docs](super#real-to-raw); the
//! refusals are [`Reject`].

use core::fmt;

use crate::diag::{Code, Diagnostic, Provenance, codes};
use crate::encoding::{Raw, RawRange};
use crate::expr::EvalError;

use super::domain::{Domain, first_true};
use super::range::{EXHAUSTIVE_LIMIT, Undefined};
use super::reject::{NonFinite, NotInvertible, Reject, write_raw};
use super::{Conversion, ExprForm, Form, Linear, ToRealError};

impl Conversion {
    /// The raw value to store for the real value `target` in a cell whose
    /// raw values are `raw`, or why it cannot be written: see
    /// [`Form::to_raw_checked`].
    ///
    /// # Errors
    ///
    /// As [`Form::to_raw_checked`].
    pub fn to_raw_checked(&self, target: f64, raw: RawRange) -> Result<Raw, Reject> {
        self.form.to_raw_checked(target, raw)
    }

    /// The inverse over `raw`, for many values: see [`Form::inverse`].
    ///
    /// # Errors
    ///
    /// As [`Form::inverse`].
    pub fn inverse(&self, raw: RawRange) -> Result<Inverse<'_>, Reject> {
        self.form.inverse(raw)
    }
}

impl Form {
    /// The `m` and `b` of a linear form: a [`Form::Linear`], or a
    /// [`Form::Expr`] whose `to_real` is linear ([`ExprForm::linear`]).
    #[must_use]
    pub fn linear(&self) -> Option<Linear> {
        match self {
            Self::Linear(l) => Some(*l),
            Self::Expr(e) => e.linear(),
            Self::Lut(_) => None,
        }
    }

    /// Edit validation: the raw value to store for the real value `target`
    /// in a cell whose raw values are `raw` (the cell encoding's
    /// [`raw_range`](crate::encoding::Encoding::raw_range)), or the
    /// [`Reject`] saying why it cannot be written. The raw value is the
    /// storable one whose real value is nearest `target` (see the
    /// [module docs](super#real-to-raw) for the rule, ties and guarantees);
    /// it is always inside `raw`, and a value the cell cannot hold is
    /// refused, never wrapped or clamped (EC-08). The same as
    /// `self.inverse(raw)?.to_raw_checked(target)`.
    ///
    /// # Errors
    ///
    /// As [`inverse`](Self::inverse) (the conversion cannot be inverted
    /// over `raw`: [`Reject::NotInvertible`], [`Reject::Singular`]), then as
    /// [`Inverse::to_raw_checked`] (this value cannot be written).
    pub fn to_raw_checked(&self, target: f64, raw: RawRange) -> Result<Raw, Reject> {
        self.inverse(raw)?.to_raw_checked(target)
    }

    /// The inverse of the conversion over the raw values in `raw`, ready to
    /// turn real values into raw ones ([`Inverse::to_raw_checked`]).
    ///
    /// A linear form ([`linear`](Self::linear)) is inverted exactly from
    /// its coefficients. Any other form (a non-linear formula, a lookup
    /// table) is solved numerically, which needs it to be monotonic over
    /// `raw`: this is checked here, once, with [`Form::real_range`]
    /// (exhaustively up to [`EXHAUSTIVE_LIMIT`] raw values, sampled above),
    /// and the raw values with a real value are located. Building it costs
    /// that scan; each [`Inverse::to_raw_checked`] is then a bisection. It
    /// holds no state between calls, so building it once for many values
    /// gives exactly the answers [`to_raw_checked`](Self::to_raw_checked)
    /// gives one at a time.
    ///
    /// # Errors
    ///
    /// The conversion cannot be inverted over `raw`, whatever the value:
    ///
    /// * [`Reject::Singular`]: every raw value has the same real value;
    /// * [`NotInvertible::NotMonotonic`]: the conversion rises and falls;
    /// * [`NotInvertible::Gap`]: a raw value with no real value lies
    ///   between two that have one;
    /// * [`NotInvertible::NoRange`]: no raw value has a real value.
    pub fn inverse(&self, raw: RawRange) -> Result<Inverse<'_>, Reject> {
        let d = Domain::new(raw);
        if let Some(line) = self.linear() {
            if line.m() == 0.0 {
                return Err(Reject::Singular { real: line.b() });
            }
            return Ok(Inverse {
                form: self,
                d,
                run: (d.lo(), d.hi()),
                increasing: line.m().is_sign_positive(),
                method: Method::Linear(line),
            });
        }
        let range = self.real_range(raw).map_err(NotInvertible::NoRange)?;
        let shape = range.shape();
        if let Some(at) = shape.gap {
            return Err(NotInvertible::Gap { at }.into());
        }
        let (increasing, trend) = match (shape.rise, shape.fall) {
            (Some(rise), Some(fall)) => {
                return Err(NotInvertible::NotMonotonic { rise, fall }.into());
            }
            (Some(rise), None) => (true, rise),
            (None, Some(fall)) => (false, fall),
            (None, None) => {
                return Err(Reject::Singular {
                    real: range.min().real(),
                });
            }
        };
        // The ends of the run of raw values with a real value: `first` and
        // `last` when every raw value was examined, otherwise found by
        // bisection between them and the nearest raw value examined that
        // has none (which bounds the bisection: it is the answer when no
        // raw value nearer has none either).
        let defined = |i: i128| self.to_real(d.value(i)).is_ok();
        let (first, last) = (d.index(shape.first), d.index(shape.last));
        let lo = shape
            .before
            .map_or(first, |b| first_true(d.index(b), first, defined));
        let hi = shape
            .after
            .map_or(last, |a| first_true(last, d.index(a), |i| !defined(i)) - 1);
        Ok(Inverse {
            form: self,
            d,
            run: (lo, hi),
            increasing,
            method: Method::Solve { trend },
        })
    }
}

/// A conversion's inverse over one raw range ([`Form::inverse`]): turns
/// real values into the raw values to store. A plain value: it holds what
/// [`Form::inverse`] found and nothing else, and remembers nothing between
/// calls.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Inverse<'a> {
    form: &'a Form,
    d: Domain,
    /// The indices with a real value (the whole domain for a linear form).
    run: (i128, i128),
    increasing: bool,
    method: Method,
}

/// How an [`Inverse`] finds a raw value.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Method {
    /// From the coefficients' estimate (bisection only as a fallback).
    Linear(Linear),
    /// By bisection, then a check of the raw values around the answer;
    /// `trend` is a raw value where the conversion moves its way.
    Solve { trend: Raw },
}

impl Inverse<'_> {
    /// Edit validation for one value: the raw value to store for `target`,
    /// the storable raw value whose real value is nearest (see the
    /// [module docs](super#real-to-raw)), always inside the raw range; or
    /// why it cannot be written. Never wraps or clamps.
    ///
    /// # Errors
    ///
    /// * [`NonFinite::Target`]: `target` is NaN or infinite;
    /// * [`Reject::OutOfRange`]: the cell cannot hold a raw value that
    ///   near `target` (more than half a raw step past either end);
    /// * [`NonFinite::Beyond`]: `target` is past the last real value on its
    ///   side, where the next raw value has none (a division by zero, an
    ///   overflow);
    /// * [`NonFinite::NoValue`]: no raw value near the estimate has a real
    ///   value (a linear form that overflows there);
    /// * for a solved form over a sampled range, [`NotInvertible::Gap`] or
    ///   [`NotInvertible::NotMonotonic`] when the raw values around the
    ///   answer show what the sample missed.
    pub fn to_raw_checked(&self, target: f64) -> Result<Raw, Reject> {
        if !target.is_finite() {
            return Err(NonFinite::Target(target).into());
        }
        let f = |x: f64| self.form.to_real(x);
        let (estimate, trend) = match self.method {
            Method::Linear(line) => (Some(estimate(line, target)), None),
            Method::Solve { trend } => (None, Some(trend)),
        };
        let solved = solve(self.d, self.run, &f, target, self.increasing, estimate)?;
        if let Some(trend) = trend {
            verify(self.d, self.run, &f, solved.index, self.increasing, trend)?;
        }
        Ok(self.d.raw(solved.index))
    }
}

impl ExprForm {
    /// The `m` and `b` of the `to_real` formula, if it is linear in `x`
    /// ([module docs](super#linear-formulas)); `m` is `0` for a formula with no
    /// `x`. Found when the form is built, never per call.
    #[must_use]
    pub const fn linear(&self) -> Option<Linear> {
        self.linear
    }

    /// Where the definition's `to_raw` formula does not invert `to_real`
    /// over the integer raw range `raw`: the lowest raw value checked whose
    /// real value the `to_raw` formula, rounded half up as legacy did
    /// (`maps/DataCell.java:447-449`), turns into a raw value that does not
    /// display the same. `None` when there is no `to_raw`, when it agrees
    /// everywhere checked, and for a float range (legacy stored a float
    /// unrounded, and an ulp of difference there is not a mismatch anyone
    /// can see).
    ///
    /// Every storable raw value is checked when there are at most
    /// [`EXHAUSTIVE_LIMIT`] of them, otherwise an even grid of that many
    /// (ends included). Raw values with no real value are skipped.
    #[must_use]
    pub fn to_raw_mismatch(&self, raw: RawRange) -> Option<ToRawMismatch> {
        let inverse = self.to_raw.as_ref()?;
        raw.as_int()?;
        let d = Domain::new(raw);
        let check = |i: i128| {
            let x = d.value(i);
            let real = self.to_real(x).ok()?;
            let given = round_half_up(match inverse.eval(real, &[]) {
                Ok(v) | Err(EvalError::NonFinite(v)) => v,
                Err(_) => f64::NAN,
            });
            let same = given == x || self.to_real(given) == Ok(real);
            (!same).then(|| ToRawMismatch {
                raw: d.raw(i),
                real,
                given,
            })
        };
        // `points` evenly spread indices, ends included: every index when
        // the domain has no more than that.
        let points = d.len().min(EXHAUSTIVE_LIMIT);
        let gaps = i128::try_from(points).unwrap_or(i128::MAX) - 1;
        let span = d.hi() - d.lo();
        (0..=gaps).find_map(|k| check(d.lo() + span * k / gaps.max(1)))
    }
}

/// `(real - b) / m`: where a linear form's raw value for `real` is,
/// approximately.
fn estimate(line: Linear, real: f64) -> f64 {
    (real - line.b()) / line.m()
}

/// Java's `Math.round` on a double, as a double: the nearest integer, a
/// half rounded up (towards `+inf`). NaN and infinities are kept.
fn round_half_up(v: f64) -> f64 {
    let down = v.floor();
    // Exact: `v - floor(v)` is in `[0, 1)` and needs no rounding.
    if v - down >= 0.5 { down + 1.0 } else { down }
}

/// A raw value whose real value a definition's `to_raw` formula does not
/// turn back into it ([`ExprForm::to_raw_mismatch`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ToRawMismatch {
    raw: Raw,
    real: f64,
    given: f64,
}

impl ToRawMismatch {
    /// The raw value.
    #[must_use]
    pub const fn raw(&self) -> Raw {
        self.raw
    }

    /// Its real value.
    #[must_use]
    pub const fn real(&self) -> f64 {
        self.real
    }

    /// What the `to_raw` formula gives for that real value, rounded half
    /// up (NaN or infinite if the formula is).
    #[must_use]
    pub const fn given(&self) -> f64 {
        self.given
    }

    /// The registry code this is reported with.
    #[must_use]
    pub const fn code(&self) -> Code {
        codes::TO_RAW_MISMATCH
    }

    /// This as a [`Warning`](crate::diag::Severity::Warning) diagnostic at
    /// `provenance`, with [`code`](Self::code) and the `Display` message.
    #[must_use]
    pub fn to_diagnostic(&self, provenance: Provenance) -> Diagnostic {
        Diagnostic::warning(self.code(), self.to_string(), provenance)
    }
}

impl fmt::Display for ToRawMismatch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("the to_raw formula does not invert the to_real formula: raw value ")?;
        write_raw(f, self.raw)?;
        write!(
            f,
            " converts to {}, which to_raw turns into {}; edits use the inverse of to_real \
             instead",
            self.real, self.given
        )
    }
}

type Eval<'a> = dyn Fn(f64) -> Result<f64, ToRealError> + 'a;

/// What [`solve`] found: the index of the raw value, and whether the
/// estimate's window had to be replaced by a bisection (always, with no
/// estimate).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Solved {
    index: i128,
    bisected: bool,
}

/// The index of the storable raw value whose real value `f` is nearest `t`
/// (see the [module docs](super#real-to-raw)), among the indices `run`
/// of the domain `d`, for an `f` monotonic over them (rising when
/// `increasing`): from the window around a linear form's `estimate` when
/// that brackets `t`, otherwise by bisection.
fn solve(
    d: Domain,
    run: (i128, i128),
    f: &Eval<'_>,
    t: f64,
    increasing: bool,
    estimate: Option<f64>,
) -> Result<Solved, Reject> {
    let (first, last) = run;
    // Real values in the direction raw values go: `key` rises with the
    // index whichever way `f` goes.
    let key = |v: f64| if increasing { v } else { -v };
    // A real value to compare, overflow included (±inf orders correctly);
    // NaN compares false everywhere.
    let at = |i: i128| match f(d.value(i)) {
        Ok(v) | Err(ToRealError::NonFinite(v)) => v,
        Err(_) => f64::NAN,
    };
    // Candidate *indices* kept inside the run: which raw values are looked
    // at, never the answer itself, which the half-step rule below refuses
    // when `t` lies past an end.
    let within_run = |i: i128| i.clamp(first, last); // no-wrap: candidate index, not a value
    // The storable values from one below the estimate's floor to two
    // above it: they bracket `t` unless the estimate is off by more than a
    // raw step.
    let window = estimate.map(|e| {
        let k = d.floor(e).unwrap_or(d.lo());
        [k - 1, k, k + 1, k + 2].map(within_run)
    });
    let bracketed = |w: [i128; 4]| {
        (w[0] == first || key(at(w[0])) <= key(t)) && (w[3] == last || key(at(w[3])) >= key(t))
    };
    let (candidates, bisected) = match window {
        Some(w) if bracketed(w) => (w, false),
        // Otherwise the bracket itself, by bisection: the last raw value
        // below `t` and the first at or above it (repeated to fill the
        // array); `last` and the one before it when every value is below.
        _ => {
            let above = first_true(first, last, |i| key(at(i)) >= key(t));
            ([above - 1, above, above, above].map(within_run), true)
        }
    };
    // The nearest, a later (larger) raw value winning a tie.
    let mut best: Option<(i128, f64, Dist)> = None;
    for i in candidates {
        let Ok(v) = f(d.value(i)) else { continue };
        let dist = distance(t, v);
        if best.is_none_or(|(_, _, b)| !b.less(dist)) {
            best = Some((i, v, dist));
        }
    }
    let Some((index, real, dist)) = best else {
        return Err(NonFinite::NoValue { target: t }.into());
    };
    let solved = Solved { index, bisected };
    // Past the last value on t's side (the end of the range, or where the
    // conversion has no value): refused unless within half the step back
    // to the other neighbour, a tie going to the larger raw value.
    let up = key(t) > key(real);
    let (toward, back) = if up {
        (index + 1, index - 1)
    } else {
        (index - 1, index + 1)
    };
    let value = |i: i128| (d.lo()..=d.hi()).contains(&i).then(|| f(d.value(i)));
    let beyond = match value(toward) {
        Some(Ok(_)) => return Ok(solved),
        Some(Err(error)) => Some(Undefined::new(d.raw(toward), error)),
        None => None,
    };
    let twice = dist.double();
    let inside = value(back)
        .and_then(Result::ok)
        .map(|n| distance(real, n))
        .is_some_and(|step| {
            if up {
                twice.less(step)
            } else {
                !step.less(twice)
            }
        });
    if inside {
        return Ok(solved);
    }
    let raw = d.raw(index);
    Err(match beyond {
        Some(beyond) => NonFinite::Beyond {
            target: t,
            limit: real,
            raw,
            beyond,
        }
        .into(),
        None => {
            let far = far_end(d, run, f, index);
            Reject::OutOfRange {
                target: t,
                min: real.min(far),
                max: real.max(far),
                nearest: raw,
            }
        }
    })
}

/// The real value at the other end of the raw values with one, from
/// `near`, an end of the domain `d` that has one (a value refused as out of
/// range is past it): with [`Reject::OutOfRange`]'s `near` end, the range
/// of real values the cell can hold. The end of `run` when it has a real
/// value, otherwise (a linear form that overflows there) the last raw value
/// towards it that has one, by bisection: a monotonic conversion has a real
/// value on one run of raw values.
fn far_end(d: Domain, run: (i128, i128), f: &Eval<'_>, near: i128) -> f64 {
    let defined = |i: i128| f(d.value(i)).is_ok();
    let far = if near == d.lo() {
        first_true(near, run.1 + 1, |i| !defined(i)) - 1
    } else {
        first_true(run.0, near, defined)
    };
    f(d.value(far)).unwrap_or(f64::NAN)
}

/// The raw values a solved answer is checked around: this many on each
/// side of it.
const VERIFY: i128 = 2;

/// Checks the raw values within [`VERIFY`] of the answer `index` (inside
/// `run`): each has a real value, and they move as the conversion does
/// (`increasing`, never the other way). After a sampled scan this is what
/// guarantees the answer is the nearest among them — the bracket the
/// bisection ended in and each side of it — whatever the sample missed
/// elsewhere; after an exhaustive one it cannot fail. `trend` is a raw
/// value where the conversion was seen to move its way, reported with a
/// turn found here.
fn verify(
    d: Domain,
    run: (i128, i128),
    f: &Eval<'_>,
    index: i128,
    increasing: bool,
    trend: Raw,
) -> Result<(), Reject> {
    let mut previous: Option<f64> = None;
    for k in -VERIFY..=VERIFY {
        let i = index + k;
        if !(run.0..=run.1).contains(&i) {
            continue;
        }
        let raw = d.raw(i);
        let v = f(d.value(i)).map_err(|error| NotInvertible::Gap {
            at: Undefined::new(raw, error),
        })?;
        if let Some(p) = previous {
            if increasing && v < p {
                return Err(NotInvertible::NotMonotonic {
                    rise: trend,
                    fall: raw,
                }
                .into());
            }
            if !increasing && v > p {
                return Err(NotInvertible::NotMonotonic {
                    rise: raw,
                    fall: trend,
                }
                .into());
            }
        }
        previous = Some(v);
    }
    Ok(())
}

/// An exact non-negative difference of two `f64`s as an unevaluated sum
/// `hi + lo` with `|lo|` at most half an ulp of `hi` (Knuth's TwoSum), so
/// two distances compare exactly. An overflowing difference is `(inf, 0)`.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Dist {
    hi: f64,
    lo: f64,
}

impl Dist {
    /// `self < other`, exactly.
    fn less(self, other: Self) -> bool {
        self.hi < other.hi || (self.hi == other.hi && self.lo < other.lo)
    }

    /// `2 * self`: exact (scaling by two only moves the exponent).
    fn double(self) -> Self {
        Self {
            hi: 2.0 * self.hi,
            lo: 2.0 * self.lo,
        }
    }
}

/// `|a - b|`, exactly, as a [`Dist`].
fn distance(a: f64, b: f64) -> Dist {
    let hi = a - b;
    if !hi.is_finite() {
        return Dist {
            hi: f64::INFINITY,
            lo: 0.0,
        };
    }
    // TwoSum(a, -b): `hi + lo == a - b` exactly.
    let bv = hi - a;
    let lo = (a - (hi - bv)) + (-b - bv);
    if hi.is_sign_negative() {
        Dist { hi: -hi, lo: -lo }
    } else {
        Dist { hi, lo }
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod solve_tests;
