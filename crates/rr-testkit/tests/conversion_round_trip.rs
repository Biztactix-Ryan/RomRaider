//! US-RR-24-1, the round-trip criterion of US-RR-24, stated in one place:
//!
//! > for every invertible conversion and every raw value `r`,
//! > `to_raw(to_real(r)) == r` exactly for linear forms and within one raw
//! > step for solved forms.
//!
//! **Every invertible conversion** is generated in three families, each over
//! every cell encoding ([`cell_encoding`]: `u8`/`i8` up to `u64`/`i64`,
//! `u20`/`u24`, `f32`, `f64`, both byte orders, unsigned and signed
//! bitfields with and without holes):
//!
//! * **linear**: `m * x + b` with `m` negative or positive, fractional,
//!   a power of two or a non-binary decimal (`0.1`, `0.01`, `1.8`), and `b`
//!   of either sign, written as a [`FormSpec::Linear`] or as formula text in
//!   the shapes definitions use (`x*m+b`, `(x+c)/d`, `b-x*m`, `-(x*m)+b`,
//!   ...). The formula must be *detected* as linear
//!   ([`Form::linear`](rr_core::conversion::Form::linear)), so it takes the
//!   exact inverse;
//! * **solved**: formulas that are monotonic over the encoding's raw range
//!   by construction but not linear (`sqrt`, `ln`, `exp`, `pow`, `x^2` and
//!   `x^3` on a non-negative shift `u = x - min`, `K/(u+c)`, the odd `x^3`
//!   and `x*abs(x)` for any range, `14.7/(1+u/c)`), increasing and
//!   decreasing, and the stepped `u-u%c` (plateaus by design);
//! * **lookup tables**: strictly increasing or decreasing values, points
//!   spanning the raw range or inset from either end (so the ends of the run
//!   with a real value are inside the range).
//!
//! **The property**, per conversion and raw range: invertibility is decided
//! by the API ([`Conversion::inverse`]); a linear form must be invertible,
//! and a solved form or table may only be refused as constant or valueless
//! (never `NotMonotonic` or `Gap`: they are monotonic and hole-free by
//! construction). Then for raw values `r` — the two ends of the raw range
//! and the two ends of the run with a real value always, plus uniform picks,
//! picks near the ends and [`value_for`]'s edge values — whose real value
//! exists:
//!
//! * `to_raw(to_real(r))` is not refused, and the reusable inverse and the
//!   one-shot [`Conversion::to_raw_checked`] agree;
//! * **linear**: it is `r`, exactly;
//! * **solved, table**: it is within one raw step of `r` — one index of
//!   the storable raw values in order, so a bitfield's holes are not steps —
//!   and (the stronger guarantee the module documents) `r` itself.
//!
//! **Plateaus** are handled explicitly, not excluded: when a neighbour of
//! `r` shares its real value (rounding, `f64` resolution beyond `2^53`, or
//! a stepped formula), "the" raw value is not unique, and the answer must
//! display as `r` does with every raw value between the two sharing it
//! (the plateau `r` is on), whatever its length.
//!
//! The raw index here ([`Grid`]) is written independently of rr-core's own
//! domain indexing, from the encoding's documented [`RawRange`] rules.
//!
//! Default case counts keep CI fast; `PROPTEST_CASES=10000` shakes out rare
//! failures.

use proptest::prelude::*;
use proptest::sample::select;
use proptest::test_runner::TestCaseError;
use rr_core::conversion::{Conversion, ConversionSpec, FormSpec, NotInvertible, Reject};
use rr_core::encoding::{Encoding, Raw, RawRange};
use rr_testkit::strategies::encoding::{cell_encoding, value_for};

// ---------------------------------------------------------------------------
// The storable raw values in order (the test's own index)
// ---------------------------------------------------------------------------

/// The storable raw values of a [`RawRange`], numbered with consecutive
/// integers lowest first: an integer by its rank among the storable
/// integers (its non-hole bits packed together), a float by its position in
/// the IEEE 754 total order (`-0.0` and `+0.0` are neighbours).
#[derive(Debug, Clone, Copy, PartialEq)]
enum Grid {
    Int { holes: u64, lo: i128, hi: i128 },
    F32 { lo: i128, hi: i128 },
    F64 { lo: i128, hi: i128 },
}

impl Grid {
    fn new(raw: RawRange) -> Self {
        match raw {
            RawRange::Int(r) => Self::Int {
                holes: r.holes(),
                lo: rank(r.min(), r.holes()),
                hi: rank(r.max(), r.holes()),
            },
            RawRange::F32 { min, max } => Self::F32 {
                lo: key32(min),
                hi: key32(max),
            },
            RawRange::F64 { min, max } => Self::F64 {
                lo: key64(min),
                hi: key64(max),
            },
        }
    }

    const fn lo(self) -> i128 {
        match self {
            Self::Int { lo, .. } | Self::F32 { lo, .. } | Self::F64 { lo, .. } => lo,
        }
    }

    const fn hi(self) -> i128 {
        match self {
            Self::Int { hi, .. } | Self::F32 { hi, .. } | Self::F64 { hi, .. } => hi,
        }
    }

    fn contains(self, i: i128) -> bool {
        (self.lo()..=self.hi()).contains(&i)
    }

    fn raw(self, i: i128) -> Raw {
        match self {
            Self::Int { holes, .. } => Raw::Int(unrank(i, holes)),
            Self::F32 { .. } => Raw::F32(from_key32(i)),
            Self::F64 { .. } => Raw::F64(from_key64(i)),
        }
    }

    /// The index of a storable raw value; `None` for a NaN or infinity.
    fn index(self, raw: Raw) -> Option<i128> {
        match (self, raw) {
            (Self::Int { holes, .. }, Raw::Int(v)) => Some(rank(v, holes)),
            (Self::F32 { .. }, Raw::F32(f)) if f.is_finite() => Some(key32(f)),
            (Self::F64 { .. }, Raw::F64(f)) if f.is_finite() => Some(key64(f)),
            _ => None,
        }
    }

    /// The raw value at the lowest index, as the `f64` a formula sees.
    fn lo_value(self) -> f64 {
        raw_f64(self.raw(self.lo()))
    }

    /// The same, highest.
    fn hi_value(self) -> f64 {
        raw_f64(self.raw(self.hi()))
    }
}

#[expect(
    clippy::cast_precision_loss,
    reason = "a raw integer as the f64 a conversion takes"
)]
fn raw_f64(raw: Raw) -> f64 {
    match raw {
        Raw::Int(v) => v as f64,
        Raw::F32(f) => f64::from(f),
        Raw::F64(f) => f,
    }
}

/// The rank of the storable integer `v`: its low 64 bits with the `holes`
/// bits removed and the rest packed down, the bits above (the sign)
/// following.
fn rank(v: i128, holes: u64) -> i128 {
    let kept = !holes;
    let mut out = 0_i128;
    let mut n = 0_u32;
    for bit in 0..64 {
        if kept >> bit & 1 == 1 {
            out |= (v >> bit & 1) << n;
            n += 1;
        }
    }
    out | ((v >> 64) << n)
}

/// The storable integer of rank `i` (the inverse of [`rank`]).
fn unrank(i: i128, holes: u64) -> i128 {
    let kept = !holes;
    let mut out = 0_i128;
    let mut n = 0_u32;
    for bit in 0..64 {
        if kept >> bit & 1 == 1 {
            out |= (i >> n & 1) << bit;
            n += 1;
        }
    }
    out | ((i >> n) << 64)
}

/// The position of a finite `f64` in the IEEE 754 total order, `+0.0` at
/// 0 and `-0.0` at -1.
fn key64(f: f64) -> i128 {
    let bits = f.to_bits();
    let magnitude = i128::from(bits & !(1 << 63));
    if bits >> 63 == 0 {
        magnitude
    } else {
        -magnitude - 1
    }
}

/// The same for an `f32`.
fn key32(f: f32) -> i128 {
    let bits = f.to_bits();
    let magnitude = i128::from(bits & !(1 << 31));
    if bits >> 31 == 0 {
        magnitude
    } else {
        -magnitude - 1
    }
}

#[expect(
    clippy::unwrap_used,
    reason = "test-only helper outside a #[test] fn; panicking is how it fails the calling test"
)]
fn from_key32(k: i128) -> f32 {
    let (magnitude, sign) = if k >= 0 {
        (k, 0)
    } else {
        (-k - 1, 1_u32 << 31)
    };
    f32::from_bits(u32::try_from(magnitude).unwrap() | sign)
}

#[expect(
    clippy::unwrap_used,
    reason = "test-only helper outside a #[test] fn; panicking is how it fails the calling test"
)]
fn from_key64(k: i128) -> f64 {
    let (magnitude, sign) = if k >= 0 {
        (k, 0)
    } else {
        (-k - 1, 1_u64 << 63)
    };
    f64::from_bits(u64::try_from(magnitude).unwrap() | sign)
}

/// The first index in `lo..end` where `pred` holds, or `end` (for a `pred`
/// false then true).
fn first_true(mut lo: i128, mut end: i128, pred: impl Fn(i128) -> bool) -> i128 {
    while lo < end {
        let mid = lo + (end - lo) / 2;
        if pred(mid) {
            end = mid;
        } else {
            lo = mid + 1;
        }
    }
    lo
}

#[test]
fn the_grid_numbers_every_storable_value_in_order() {
    use rr_core::encoding::{BitField, Storage};
    // A bitfield with holes: mask 0b1011_0100 keeps bits 2, 4, 5, 7.
    let raw = RawRange::Int(
        BitField::new(0b1011_0100, Some(0), false)
            .unwrap()
            .raw_range(),
    );
    let g = Grid::new(raw);
    let RawRange::Int(r) = raw else {
        unreachable!()
    };
    let storable: Vec<i128> = (r.min()..=r.max()).filter(|&v| r.contains(v)).collect();
    let indexed: Vec<i128> = (g.lo()..=g.hi())
        .map(|i| match g.raw(i) {
            Raw::Int(v) => v,
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(indexed, storable);
    // Signed storage: every value, ends included.
    let g = Grid::new(Storage::I16.raw_range());
    assert_eq!(g.raw(g.lo()), Raw::Int(-32768));
    assert_eq!(g.raw(g.hi()), Raw::Int(32767));
    assert_eq!(g.hi() - g.lo(), 65535);
    // Floats: -0.0 and +0.0 are neighbours, the ends are -MAX and MAX.
    let g = Grid::new(Storage::F32.raw_range());
    assert_eq!(
        g.index(Raw::F32(0.0)),
        Some(g.index(Raw::F32(-0.0)).unwrap() + 1)
    );
    assert_eq!(g.raw(g.lo()), Raw::F32(-f32::MAX));
    assert_eq!(g.raw(g.hi()), Raw::F32(f32::MAX));
    assert_eq!(g.raw(1), Raw::F32(f32::from_bits(1)));
    let g = Grid::new(Storage::F64.raw_range());
    assert_eq!(g.raw(g.lo()), Raw::F64(-f64::MAX));
    assert_eq!(g.index(g.raw(-5)), Some(-5));
}

// ---------------------------------------------------------------------------
// Conversions
// ---------------------------------------------------------------------------

/// Which guarantee a conversion has.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Family {
    /// Inverted exactly: `to_raw(to_real(r)) == r`.
    Linear,
    /// Solved numerically: within one raw step of `r`.
    Solved,
    /// A lookup table, solved numerically: within one raw step of `r`.
    Table,
}

/// A conversion as a definition writes it.
#[derive(Debug, Clone, PartialEq)]
enum Source {
    Linear { m: f64, b: f64 },
    Expr(String),
    Lut(Vec<(f64, f64)>),
}

impl Source {
    #[expect(
        clippy::panic,
        reason = "test-only helper outside a #[test] fn; panicking is how it fails the calling test"
    )]
    fn build(&self) -> Conversion {
        let form = match self {
            Self::Linear { m, b } => FormSpec::Linear { m: *m, b: *b },
            Self::Expr(text) => FormSpec::Expr {
                to_real: text,
                to_raw: None,
            },
            Self::Lut(points) => FormSpec::Lut(points),
        };
        Conversion::from_spec(&ConversionSpec {
            form,
            unit: None,
            format: None,
        })
        .unwrap_or_else(|e| panic!("{self:?} does not build: {e}"))
    }
}

/// A constant as formula text, read back exactly; a negative one in
/// parentheses (`x*(-0.1)`).
fn lit(v: f64) -> String {
    if v.is_sign_negative() {
        format!("({v:?})")
    } else {
        format!("{v:?}")
    }
}

/// A factor's size: the decimals definitions use (not binary fractions),
/// powers of two, whole numbers.
fn magnitude() -> BoxedStrategy<f64> {
    prop_oneof![
        select(vec![
            0.1,
            0.01,
            0.001,
            0.3,
            1.8,
            0.25,
            0.5,
            0.0625,
            1.0 / 256.0,
            1.0 / 65536.0,
            1.0,
            2.0,
            3.0,
            10.0,
            100.0,
            1000.0,
        ]),
        (1_u32..=100_000).prop_map(|n| f64::from(n) / 1000.0),
        (1_u32..=4096).prop_map(|n| f64::from(n) / 65536.0),
        (1_u32..=1000).prop_map(f64::from),
    ]
    .boxed()
}

/// A non-zero factor of either sign.
fn factor() -> BoxedStrategy<f64> {
    (magnitude(), any::<bool>())
        .prop_map(|(v, neg)| if neg { -v } else { v })
        .boxed()
}

/// An offset of either sign.
fn offset() -> BoxedStrategy<f64> {
    prop_oneof![
        Just(0.0),
        select(vec![-40.0, 273.15, 14.7, -1.5, 32.0, -0.5]),
        (-100_000_i32..=100_000).prop_map(|n| f64::from(n) / 100.0),
    ]
    .boxed()
}

/// A linear conversion, as a [`FormSpec::Linear`] or as formula text of
/// one of the shapes definitions use.
fn linear() -> BoxedStrategy<Source> {
    (0_u8..9, factor(), offset(), offset(), factor())
        .prop_map(|(shape, m, b, c, d)| {
            let (lm, lb, lc, ld) = (lit(m), lit(b), lit(c), lit(d));
            Source::Expr(match shape {
                0 => format!("x*{lm}+{lb}"),
                1 => format!("(x+{lc})/{ld}"),
                2 => format!("{lb}-x*{lm}"),
                3 => format!("{lm}*x-{lb}"),
                4 => format!("x/{ld}+{lb}"),
                5 => format!("(x-{lc})*{lm}"),
                6 => format!("-(x*{lm})+{lb}"),
                7 => format!("{lb}+{lm}*(x+{lc})"),
                _ => return Source::Linear { m, b },
            })
        })
        .boxed()
}

/// A non-linear conversion monotonic over `g` by construction: increasing
/// or decreasing functions of `u = x - min` (never negative over the
/// range), odd functions of `x`, and a stepped one.
fn solved(g: Grid) -> BoxedStrategy<Source> {
    let u = format!("(x-{})", lit(g.lo_value()));
    let c = (1_u32..=1000).prop_map(f64::from);
    let p = select(vec![0.5, 1.5, 2.5, 3.0]);
    (0_u8..12, factor(), offset(), c, p)
        .prop_map(move |(shape, k, b, c, p)| {
            let (k, b, c, p) = (lit(k), lit(b), lit(c), lit(p));
            Source::Expr(match shape {
                0 => format!("sqrt({u})*{k}+{b}"),
                1 => format!("{u}*{u}*{k}+{b}"),
                2 => format!("{c}*1000/({u}+{c})"),
                3 => format!("ln({u}+{c})*{k}"),
                4 => format!("exp({u}/{c})*{k}"),
                5 => format!("{u}*{u}*{u}/{c}-{b}"),
                6 => format!("{b}-sqrt({u})*{k}"),
                7 => format!("x*x*x*{k}"),
                8 => format!("x*abs(x)*{k}"),
                9 => format!("14.7/(1+{u}/{c})"),
                10 => format!("pow({u},{p})*{k}+{b}"),
                _ => format!("{u}-{u}%{c}"),
            })
        })
        .boxed()
}

/// A lookup table with strictly monotonic values whose points span `g`
/// (within `±1e300`, so the span stays finite), or are inset from either
/// end.
fn table(g: Grid) -> BoxedStrategy<Source> {
    let lo = g.lo_value().max(-1e300);
    let hi = g.hi_value().min(1e300);
    (
        prop::collection::vec(1_u32..(1 << 20), 0..5),
        any::<bool>(),
        any::<bool>(),
        -1000_i32..=1000,
        prop::collection::vec(1_u32..=10_000, 7),
        any::<bool>(),
    )
        .prop_map(move |(mut inner, inset_lo, inset_hi, start, steps, down)| {
            let span = hi - lo;
            let at = |f: f64| lo + span * f;
            inner.sort_unstable();
            let mut xs = vec![if inset_lo { at(0.01) } else { lo }];
            xs.extend(
                inner
                    .iter()
                    .map(|&f| at(f64::from(f) / f64::from(1_u32 << 20))),
            );
            xs.push(if inset_hi { at(0.99) } else { hi });
            xs.dedup_by(|b, a| b <= a);
            if xs.len() < 2 {
                xs = vec![lo, hi];
            }
            let mut y = f64::from(start) / 10.0;
            let points = xs
                .iter()
                .zip(steps.iter().cycle())
                .map(|(&x, &s)| {
                    let point = (x, y);
                    let s = f64::from(s) / 100.0;
                    y = if down { y - s } else { y + s };
                    point
                })
                .collect();
            Source::Lut(points)
        })
        .boxed()
}

// ---------------------------------------------------------------------------
// The property
// ---------------------------------------------------------------------------

/// An encoding with a single-cell raw range, raw values it can store
/// ([`value_for`]: uniform and edge values) and index picks.
fn encoding() -> impl Strategy<Value = (Encoding, Vec<Raw>, Vec<(u8, u64)>)> {
    cell_encoding().prop_flat_map(|e| {
        (
            Just(e),
            prop::collection::vec(value_for(&e), 8),
            prop::collection::vec((any::<u8>(), any::<u64>()), 8),
        )
    })
}

#[expect(
    clippy::unwrap_used,
    reason = "test-only helper outside a #[test] fn; panicking is how it fails the calling test"
)]
fn raw_range(e: Encoding) -> RawRange {
    e.raw_range().unwrap()
}

/// An index of `g` from a pick: uniform, within 64 of either end, or
/// within 2048 of raw value 0.
#[expect(
    clippy::unwrap_used,
    reason = "test-only helper outside a #[test] fn; panicking is how it fails the calling test"
)]
fn pick(g: Grid, (zone, n): (u8, u64)) -> i128 {
    let span = u128::try_from(g.hi() - g.lo()).unwrap() + 1;
    let off = |m: u128| i128::try_from(m % span).unwrap();
    match zone % 4 {
        0 => g.lo() + off(u128::from(n)),
        1 => g.lo() + off(u128::from(n % 64)),
        2 => g.hi() - off(u128::from(n % 64)),
        _ => {
            let zero = first_true(g.lo(), g.hi(), |i| raw_f64(g.raw(i)) >= 0.0);
            (zero + i128::from(n % 4096) - 2048).clamp(g.lo(), g.hi())
        }
    }
}

/// The longest plateau walked index by index; a longer one (an integer
/// beyond `2^53` under a small factor) is checked at both ends.
const WALK: i128 = 1 << 12;

/// The round-trip criterion for one conversion over one raw range, at the
/// raw values `extra` (indices of `g`) and the ends of the range and of the
/// run with a real value.
fn round_trip(
    source: &Source,
    family: Family,
    raw: RawRange,
    extra: &[i128],
) -> Result<(), TestCaseError> {
    let c = source.build();
    let g = Grid::new(raw);
    let is_linear = c.form().linear().is_some();
    prop_assert_eq!(
        is_linear,
        family == Family::Linear,
        "{:?}: linear detection",
        source
    );

    // Invertibility, decided by the API.
    let inv = match c.inverse(raw) {
        Ok(inv) => inv,
        Err(e) => {
            prop_assert!(
                family != Family::Linear,
                "{source:?} over {raw:?}: a linear form is refused: {e}"
            );
            // Monotonic and hole-free by construction: only a run too short
            // to move (a constant), or none at all, may be refused.
            prop_assert!(
                matches!(
                    e,
                    Reject::Singular { .. } | Reject::NotInvertible(NotInvertible::NoRange(_))
                ),
                "{source:?} over {raw:?}: a monotonic conversion is refused: {e}"
            );
            return Ok(());
        }
    };

    let real_at = |i: i128| c.to_real_raw(g.raw(i));
    let defined = |i: i128| real_at(i).is_ok();
    let mut points = vec![g.lo(), g.hi()];
    points.extend_from_slice(extra);
    // The ends of the run with a real value, from any raw value with one.
    if let Some(&anchor) = points.iter().find(|&&i| defined(i)) {
        points.push(first_true(g.lo(), anchor, defined));
        points.push(first_true(anchor, g.hi() + 1, |i| !defined(i)) - 1);
    }

    for (n, i) in points.into_iter().enumerate() {
        let r = g.raw(i);
        let Ok(real) = real_at(i) else { continue };
        let got = inv.to_raw_checked(real);
        // The one-shot to_raw rebuilds the inverse (a scan of the range):
        // compared at the lowest end of the range only.
        if n == 0 {
            prop_assert_eq!(
                got,
                c.to_raw_checked(real, raw),
                "{:?} over {:?}: the inverse and to_raw disagree at {:?}",
                source,
                raw,
                r
            );
        }
        let got = match got {
            Ok(got) => got,
            Err(e) => {
                return Err(TestCaseError::fail(format!(
                    "{source:?} over {raw:?}: to_raw(to_real({r:?}) = {real:?}) is refused: {e}"
                )));
            }
        };
        prop_assert!(
            raw.contains(got),
            "{source:?}: {got:?} is not storable in {raw:?}"
        );
        let Some(j) = g.index(got) else {
            return Err(TestCaseError::fail(format!(
                "{source:?}: {got:?} is not a raw value"
            )));
        };
        let shared = [i - 1, i + 1]
            .into_iter()
            .any(|n| g.contains(n) && real_at(n) == Ok(real));

        if shared {
            // A plateau: r's real value is not one raw value's. The answer
            // displays as r does, and so does every raw value between them
            // (it is on r's plateau).
            prop_assert_eq!(
                c.to_real_raw(got),
                Ok(real),
                "{:?} over {:?}: plateau at {:?}, answer {:?}",
                source,
                raw,
                r,
                got
            );
            let (a, b) = (i.min(j), i.max(j));
            if b - a <= WALK {
                for k in a..=b {
                    prop_assert_eq!(
                        real_at(k),
                        Ok(real),
                        "{:?} over {:?}: {:?} is not on the plateau of {:?}",
                        source,
                        raw,
                        got,
                        r
                    );
                }
            }
            continue;
        }

        match family {
            Family::Linear => prop_assert_eq!(
                got,
                r,
                "{:?} over {:?}: to_raw(to_real(r)) != r for the linear form, real {:?}",
                source,
                raw,
                real
            ),
            Family::Solved | Family::Table => {
                prop_assert!(
                    (j - i).abs() <= 1,
                    "{source:?} over {raw:?}: to_raw(to_real({r:?})) = {got:?}, {} raw steps away",
                    j - i
                );
                // What the module documents beyond the criterion: no
                // neighbour shares r's real value, so r is the only raw
                // value at distance 0.
                prop_assert_eq!(
                    got,
                    r,
                    "{:?} over {:?}: not r itself, real {:?}",
                    source,
                    raw,
                    real
                );
            }
        }
    }
    Ok(())
}

/// The picks and edge values as indices of `g`.
fn indices(g: Grid, values: &[Raw], picks: &[(u8, u64)]) -> Vec<i128> {
    let mut out: Vec<i128> = values.iter().filter_map(|&v| g.index(v)).collect();
    out.retain(|&i| g.contains(i));
    out.extend(picks.iter().map(|&p| pick(g, p)));
    out
}

/// `cases` cases unless `PROPTEST_CASES` says otherwise, and a shrink
/// budget: a failure reports its conversion and raw value whether or not
/// it is minimal.
fn config(cases: u32) -> ProptestConfig {
    let default = ProptestConfig::default();
    ProptestConfig {
        cases: if std::env::var_os("PROPTEST_CASES").is_some() {
            default.cases
        } else {
            cases
        },
        max_shrink_iters: 256,
        ..default
    }
}

proptest! {
    #![proptest_config(config(256))]

    /// Linear forms (coefficients or detected-linear formulas):
    /// `to_raw(to_real(r)) == r` exactly.
    #[test]
    fn linear_forms_round_trip_exactly(
        (e, values, picks) in encoding(),
        source in linear(),
    ) {
        let raw = raw_range(e);
        let g = Grid::new(raw);
        round_trip(&source, Family::Linear, raw, &indices(g, &values, &picks))?;
    }
}

proptest! {
    // Each case scans the raw range of a formula (up to 2^16 evaluations).
    #![proptest_config(config(128))]

    /// Solved (monotonic, non-linear) formulas: `to_raw(to_real(r))` is
    /// within one raw step of `r` (and is `r` off a plateau).
    #[test]
    fn solved_forms_round_trip_within_one_raw_step(
        (e, values, picks, source) in encoding().prop_flat_map(|(e, v, p)| {
            let g = Grid::new(raw_range(e));
            (Just(e), Just(v), Just(p), solved(g))
        }),
    ) {
        let raw = raw_range(e);
        let g = Grid::new(raw);
        round_trip(&source, Family::Solved, raw, &indices(g, &values, &picks))?;
    }

    /// Lookup tables with strictly monotonic values: `to_raw(to_real(r))`
    /// is within one raw step of `r` (and is `r` off a plateau).
    #[test]
    fn lookup_tables_round_trip_within_one_raw_step(
        (e, values, picks, source) in encoding().prop_flat_map(|(e, v, p)| {
            let g = Grid::new(raw_range(e));
            (Just(e), Just(v), Just(p), table(g))
        }),
    ) {
        let raw = raw_range(e);
        let g = Grid::new(raw);
        round_trip(&source, Family::Table, raw, &indices(g, &values, &picks))?;
    }
}
