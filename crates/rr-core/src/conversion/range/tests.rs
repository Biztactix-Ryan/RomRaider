//! Unit tests for display ranges (US-RR-24-10, MF-13). Every expected value
//! is hand-calculated; the working is in the comment beside it.

use super::*;
use crate::conversion::{ConversionSpec, FormSpec};
use crate::diag::{Provenance, Severity, Span};
use crate::encoding::{BitField, IntRange, Storage};
use proptest::prelude::*;

fn expr(to_real: &str) -> Conversion {
    Conversion::from_spec(&ConversionSpec {
        form: FormSpec::Expr {
            to_real,
            to_raw: None,
        },
        unit: None,
        format: None,
    })
    .unwrap()
}

fn linear(m: f64, b: f64) -> Conversion {
    Conversion::new(Form::Linear(Linear::new(m, b).unwrap()), None, None)
}

fn lut(points: &[(f64, f64)]) -> Conversion {
    Conversion::new(Form::Lut(Lut::new(points).unwrap()), None, None)
}

fn field(mask: u64, shift: Option<u32>, signed: bool) -> RawRange {
    RawRange::Int(BitField::new(mask, shift, signed).unwrap().raw_range())
}

fn range(c: &Conversion, raw: RawRange) -> RealRange {
    c.real_range(raw).unwrap()
}

/// `(min, raw at min, max, raw at max)`.
fn ends(r: &RealRange) -> (f64, Raw, f64, Raw) {
    (r.min().real(), r.min().raw(), r.max().real(), r.max().raw())
}

fn int(v: i128) -> Raw {
    Raw::Int(v)
}

/// The real value of `raw`, as `Conversion::to_real_raw` gives it.
fn at(c: &Conversion, raw: Raw) -> f64 {
    c.to_real_raw(raw).unwrap()
}

// ---------------------------------------------------------------------------
// Decreasing scalings (MF-13)
// ---------------------------------------------------------------------------

#[test]
fn a_decreasing_linear_scaling_has_its_ends_the_right_way_round() {
    // -0.5*x+100 over u8: f(0) = 100, f(255) = -127.5 + 100 = -27.5.
    // Legacy reported max = f(maxBin) = f(255) = -27.5 (MF-13).
    let r = range(&linear(-0.5, 100.0), Storage::U8.raw_range());
    assert_eq!(ends(&r), (-27.5, int(255), 100.0, int(0)));
    assert_eq!(r.monotonicity(), Monotonicity::Decreasing);
    assert_eq!(r.coverage(), Coverage::Exact);
    assert_eq!(r.undefined(), None);
}

#[test]
fn decreasing_formulas_have_their_ends_the_right_way_round() {
    for text in ["-0.5*x+100", "100-x/2"] {
        let r = range(&expr(text), Storage::U8.raw_range());
        assert_eq!(ends(&r), (-27.5, int(255), 100.0, int(0)), "{text}");
        assert_eq!(r.monotonicity(), Monotonicity::Decreasing, "{text}");
        assert_eq!(r.coverage(), Coverage::Exact, "{text}");
        assert_eq!(r.undefined(), None, "{text}");
    }
}

#[test]
fn an_increasing_linear_scaling_over_a_signed_storage() {
    // 0.25*x-50 over i16: f(-32768) = -8192 - 50 = -8242,
    // f(32767) = 8191.75 - 50 = 8141.75.
    let r = range(&linear(0.25, -50.0), Storage::I16.raw_range());
    assert_eq!(ends(&r), (-8242.0, int(-32768), 8141.75, int(32767)));
    assert_eq!(r.monotonicity(), Monotonicity::Increasing);
}

#[test]
fn a_turning_formula_finds_its_interior_extremum() {
    // (x-128)^2 over u8: f(0) = 16384, f(128) = 0, f(255) = 127^2 = 16129.
    let r = range(&expr("(x-128)^2"), Storage::U8.raw_range());
    assert_eq!(ends(&r), (0.0, int(128), 16384.0, int(0)));
    assert_eq!(r.monotonicity(), Monotonicity::NonMonotonic);
    assert_eq!(r.coverage(), Coverage::Exact);

    // Over i8 it only falls: f(-128) = 256^2 = 65536, f(127) = 1^2 = 1.
    // Legacy: min = f(minBin) = 65536, max = f(maxBin) = 1.
    let r = range(&expr("(x-128)^2"), Storage::I8.raw_range());
    assert_eq!(ends(&r), (1.0, int(127), 65536.0, int(-128)));
    assert_eq!(r.monotonicity(), Monotonicity::Decreasing);
}

#[test]
fn a_decreasing_lookup_table() {
    // (0,100) (100,50) (200,0) over u8: 100 at 0, 0 at 200; 201 is past the
    // last point.
    let r = range(
        &lut(&[(0.0, 100.0), (100.0, 50.0), (200.0, 0.0)]),
        Storage::U8.raw_range(),
    );
    assert_eq!(ends(&r), (0.0, int(200), 100.0, int(0)));
    assert_eq!(r.monotonicity(), Monotonicity::Decreasing);
    assert_eq!(r.coverage(), Coverage::Exact);
    let u = r.undefined().unwrap();
    assert_eq!(u.raw(), int(201));
    assert_eq!(
        u.error(),
        ToRealError::OutsideTable {
            raw: 201.0,
            min: 0.0,
            max: 200.0
        }
    );
}

#[test]
fn a_lookup_table_peak_between_storable_values() {
    // (0,0) (10.5,10) (20,0) over u8. The peak at 10.5 is not storable:
    // f(10) = 10 * 10/10.5 = 9.5238..., f(11) = 10 - 10 * 0.5/9.5 =
    // 9.4736..., so the maximum is at 10. f(0) = f(20) = 0: the minimum is
    // first reached at 0.
    let r = range(
        &lut(&[(0.0, 0.0), (10.5, 10.0), (20.0, 0.0)]),
        Storage::U8.raw_range(),
    );
    assert_eq!(ends(&r), (0.0, int(0), 10.0 * (10.0 / 10.5), int(10)));
    assert!((r.max().real() - 9.523_809_523_809_524).abs() < 1e-12);
    assert_eq!(r.monotonicity(), Monotonicity::NonMonotonic);
    assert_eq!(r.undefined().map(Undefined::raw), Some(int(21)));
}

#[test]
fn a_lookup_table_over_a_4_byte_storage_is_exact() {
    // (0,0) (1e6,-5) (4e9,5) over u32: -5 at 1000000, 5 at 4000000000,
    // nothing past 4000000000.
    let r = range(
        &lut(&[(0.0, 0.0), (1e6, -5.0), (4e9, 5.0)]),
        Storage::U32.raw_range(),
    );
    assert_eq!(ends(&r), (-5.0, int(1_000_000), 5.0, int(4_000_000_000)));
    assert_eq!(r.monotonicity(), Monotonicity::NonMonotonic);
    assert_eq!(r.coverage(), Coverage::Exact);
    assert_eq!(r.undefined().map(Undefined::raw), Some(int(4_000_000_001)));
}

#[test]
fn a_decreasing_lookup_table_over_a_float_storage() {
    // (-1,2) (1,-2) over f32: 2 at -1, -2 at 1; -f32::MAX is outside it.
    let r = range(&lut(&[(-1.0, 2.0), (1.0, -2.0)]), Storage::F32.raw_range());
    assert_eq!(ends(&r), (-2.0, Raw::F32(1.0), 2.0, Raw::F32(-1.0)));
    assert_eq!(r.monotonicity(), Monotonicity::Decreasing);
    assert_eq!(r.coverage(), Coverage::Exact);
    assert_eq!(r.undefined().map(Undefined::raw), Some(Raw::F32(-f32::MAX)));
}

#[test]
fn a_lookup_table_over_a_bitfield_skips_the_holes() {
    // Storable {0, 2, 8, 10}; (0,0) (5,10) (10,0): f(2) = f(8) = 4, the peak
    // at 5 is not storable.
    let r = range(
        &lut(&[(0.0, 0.0), (5.0, 10.0), (10.0, 0.0)]),
        field(0b1010, Some(0), false),
    );
    assert_eq!(ends(&r), (0.0, int(0), 4.0, int(2)));
    assert_eq!(r.monotonicity(), Monotonicity::NonMonotonic);
    assert_eq!(r.undefined(), None);
}

#[test]
fn a_lookup_table_segment_that_overflows_is_reported_and_sampled() {
    // (0,-1e308) (2,1e308): the difference overflows, so f(1) is infinite.
    let r = range(
        &lut(&[(0.0, -1e308), (2.0, 1e308)]),
        Storage::U8.raw_range(),
    );
    assert_eq!(ends(&r), (-1e308, int(0), 1e308, int(2)));
    assert_eq!(r.coverage(), Coverage::Sampled);
    let u = r.undefined().unwrap();
    assert_eq!(u.raw(), int(1));
    assert_eq!(u.error(), ToRealError::NonFinite(f64::INFINITY));
    // A table starting inside the range has no overflow and stays exact.
    let r = range(&lut(&[(1.0, 0.0), (2.0, 1.0)]), Storage::U8.raw_range());
    assert_eq!(r.coverage(), Coverage::Exact);
    assert_eq!(r.undefined().map(Undefined::raw), Some(int(0)));
}

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

#[test]
fn a_constant_reports_its_value_at_the_lowest_raw_value() {
    let r = range(&linear(0.0, 3.0), Storage::U8.raw_range());
    assert_eq!(ends(&r), (3.0, int(0), 3.0, int(0)));
    assert_eq!(r.monotonicity(), Monotonicity::Constant);
    for raw in [Storage::U8.raw_range(), Storage::U32.raw_range()] {
        let r = range(&expr("3"), raw);
        assert_eq!(ends(&r), (3.0, int(0), 3.0, int(0)));
        assert_eq!(r.monotonicity(), Monotonicity::Constant);
    }
}

// ---------------------------------------------------------------------------
// Holes in the raw range: division by zero, NaN, nothing at all
// ---------------------------------------------------------------------------

#[test]
fn a_division_by_zero_is_left_out_and_reported() {
    // 1/(x-10): f(9) = -1, f(10) = 1/0 = inf, f(11) = 1. It falls on each
    // side of 10 and jumps up across it.
    for (raw, coverage) in [
        (Storage::U8.raw_range(), Coverage::Exact),
        (Storage::U32.raw_range(), Coverage::Sampled),
        (Storage::I32.raw_range(), Coverage::Sampled),
    ] {
        let r = range(&expr("1/(x-10)"), raw);
        assert_eq!(ends(&r), (-1.0, int(9), 1.0, int(11)), "{raw:?}");
        assert_eq!(r.monotonicity(), Monotonicity::NonMonotonic);
        assert_eq!(r.coverage(), coverage);
        let u = r.undefined().unwrap();
        assert_eq!(u.raw(), int(10));
        assert_eq!(u.error(), ToRealError::NonFinite(f64::INFINITY));
    }
}

#[test]
fn values_after_a_leading_hole_still_count() {
    // sqrt(x-10) is NaN below 10, then rises: 0 at 10, sqrt(245) at 255.
    for raw in [Storage::U8.raw_range(), Storage::U32.raw_range()] {
        let r = range(&expr("sqrt(x-10)"), raw);
        assert_eq!((r.min().real(), r.min().raw()), (0.0, int(10)));
        assert_eq!(r.monotonicity(), Monotonicity::Increasing);
        let u = r.undefined().unwrap();
        assert_eq!(u.raw(), int(0));
        assert!(matches!(u.error(), ToRealError::NonFinite(v) if v.is_nan()));
    }
    let r = range(&expr("sqrt(x-10)"), Storage::U8.raw_range());
    assert_eq!(
        (r.max().real(), r.max().raw()),
        (245.0_f64.sqrt(), int(255))
    );
}

#[test]
fn no_value_anywhere_is_an_error_with_a_diagnostic() {
    // 1/(x-x) = 1/0 = inf for every x.
    for (raw, coverage) in [
        (Storage::U8.raw_range(), Coverage::Exact),
        (Storage::U32.raw_range(), Coverage::Sampled),
    ] {
        let e = expr("1/(x-x)").real_range(raw).unwrap_err();
        let RangeError::NoValue {
            lowest,
            coverage: c,
        } = e;
        assert_eq!(lowest.raw(), int(0));
        assert_eq!(lowest.error(), ToRealError::NonFinite(f64::INFINITY));
        assert_eq!(c, coverage);
    }
    // A table entirely above an 8-bit cell.
    let e = lut(&[(300.0, 1.0), (400.0, 2.0)])
        .real_range(Storage::U8.raw_range())
        .unwrap_err();
    assert_eq!(
        e,
        RangeError::NoValue {
            lowest: Undefined {
                raw: int(0),
                error: ToRealError::OutsideTable {
                    raw: 0.0,
                    min: 300.0,
                    max: 400.0
                },
            },
            coverage: Coverage::Exact,
        }
    );
    assert_eq!(e.code(), codes::CONVERSION_NO_VALUE);
    assert_eq!(e.code().as_str(), "CNV0004");
    assert_eq!(
        e.to_string(),
        "no raw value the cell can hold has a real value (raw value 0: raw value 0 is \
         outside the lookup table, which covers 300 to 400)"
    );
    let d = e.to_diagnostic(Provenance::new("defs.xml", Span::line_col(1, 1)));
    assert_eq!(d.severity(), Severity::Error);
    assert_eq!(d.code(), codes::CONVERSION_NO_VALUE);
    assert_eq!(d.message(), e.to_string());
}

#[test]
fn the_no_value_message_names_float_raw_values() {
    for (raw, text) in [
        (Raw::F32(-1.5), "raw value -1.5: "),
        (Raw::F64(2.25), "raw value 2.25: "),
    ] {
        let e = RangeError::NoValue {
            lowest: Undefined {
                raw,
                error: ToRealError::NonFinite(f64::INFINITY),
            },
            coverage: Coverage::Exact,
        };
        assert!(e.to_string().contains(text), "{e}");
    }
}

// ---------------------------------------------------------------------------
// Linear forms that overflow at the ends of a wide range
// ---------------------------------------------------------------------------

#[test]
fn an_overflowing_linear_form_stops_at_its_last_finite_value() {
    // 2*x over f64: finite exactly for |x| <= f64::MAX/2 (2 * MAX/2 = MAX;
    // the next float up doubles past it).
    let half = f64::MAX / 2.0;
    let r = range(&linear(2.0, 0.0), Storage::F64.raw_range());
    assert_eq!(
        ends(&r),
        (-f64::MAX, Raw::F64(-half), f64::MAX, Raw::F64(half))
    );
    assert_eq!(r.monotonicity(), Monotonicity::Increasing);
    assert_eq!(r.coverage(), Coverage::Exact);
    let u = r.undefined().unwrap();
    assert_eq!(u.raw(), Raw::F64(-f64::MAX));
    assert_eq!(u.error(), ToRealError::NonFinite(f64::NEG_INFINITY));

    let r = range(&linear(-2.0, 0.0), Storage::F64.raw_range());
    assert_eq!(
        ends(&r),
        (-f64::MAX, Raw::F64(half), f64::MAX, Raw::F64(-half))
    );
    assert_eq!(r.monotonicity(), Monotonicity::Decreasing);
    assert_eq!(
        r.undefined().map(Undefined::error),
        Some(ToRealError::NonFinite(f64::INFINITY))
    );
}

#[test]
fn an_overflowing_linear_form_reports_its_upper_tail() {
    // 1e300*x over u64: MAX/1e300 = 179769313.486..., so 179769313 is the
    // last raw value with a finite result and 179769314 the first without.
    let r = range(&linear(1e300, 0.0), Storage::U64.raw_range());
    assert_eq!((r.min().real(), r.min().raw()), (0.0, int(0)));
    assert_eq!(r.max().raw(), int(179_769_313));
    assert_eq!(r.max().real(), 1e300 * 179_769_313.0);
    let u = r.undefined().unwrap();
    assert_eq!(u.raw(), int(179_769_314));
    assert_eq!(u.error(), ToRealError::NonFinite(f64::INFINITY));
}

// ---------------------------------------------------------------------------
// 4-byte and wider ranges: sampled formulas
// ---------------------------------------------------------------------------

#[test]
fn a_linear_formula_over_u32_is_exact_at_its_ends() {
    // x*0.01: f(0) = 0, f(4294967295) = 42949672.95.
    let c = expr("x*0.01");
    let r = range(&c, Storage::U32.raw_range());
    assert_eq!(
        ends(&r),
        (0.0, int(0), 4_294_967_295.0 * 0.01, int(4_294_967_295))
    );
    assert!((r.max().real() - 42_949_672.95).abs() < 1e-6);
    assert_eq!(r.monotonicity(), Monotonicity::Increasing);
    assert_eq!(r.coverage(), Coverage::Sampled);
    assert_eq!(r.undefined(), None);
    // The linear form says the same, exactly.
    let l = range(&linear(0.01, 0.0), Storage::U32.raw_range());
    assert_eq!(ends(&l), ends(&r));
    assert_eq!(l.coverage(), Coverage::Exact);
}

#[test]
fn a_sampled_scan_refines_an_interior_minimum_off_the_grid() {
    // (x-1000000)^2 over u32: the grid steps 2^18 = 262144, so 1000000 is
    // no sample; the refinement lands on it exactly. The maximum is at the
    // far end: (4294967295 - 1000000)^2 = 4293967295^2.
    let c = expr("(x-1000000)^2");
    let r = range(&c, Storage::U32.raw_range());
    assert_eq!((r.min().real(), r.min().raw()), (0.0, int(1_000_000)));
    assert_eq!(r.max().raw(), int(4_294_967_295));
    assert_eq!(r.max().real(), at(&c, int(4_294_967_295)));
    assert!((r.max().real() / 1.843_814_00e19 - 1.0).abs() < 1e-6);
    assert_eq!(r.monotonicity(), Monotonicity::NonMonotonic);

    // The same over i32, where the formula peaks at the low end:
    // (-2147483648 - 1000000)^2 > (2147483647 - 1000000)^2.
    let r = range(&c, Storage::I32.raw_range());
    assert_eq!((r.min().real(), r.min().raw()), (0.0, int(1_000_000)));
    assert_eq!(r.max().raw(), int(-2_147_483_648));
}

#[test]
fn a_sampled_scan_refines_an_interior_maximum() {
    // 7-(x-3000000)^2/1e12 over i32 peaks at 7 at 3000000.
    let r = range(&expr("7-(x-3000000)^2/1e12"), Storage::I32.raw_range());
    assert_eq!((r.max().real(), r.max().raw()), (7.0, int(3_000_000)));
}

#[test]
fn a_sampled_scan_over_floats() {
    // x over f32: the ends of the finite range.
    let max = f64::from(f32::MAX);
    let r = range(&expr("x"), Storage::F32.raw_range());
    assert_eq!(
        ends(&r),
        (-max, Raw::F32(-f32::MAX), max, Raw::F32(f32::MAX))
    );
    assert_eq!(r.monotonicity(), Monotonicity::Increasing);
    assert_eq!(r.coverage(), Coverage::Sampled);

    // x*x over f64: 0 at the middle of the range (-0.0 is sampled first),
    // overflowing far out. The maximum is the last finite square: the
    // next float up squares to infinity.
    let c = expr("x*x");
    let r = range(&c, Storage::F64.raw_range());
    assert_eq!(r.min().real(), 0.0);
    let Raw::F64(peak) = r.max().raw() else {
        panic!("an F64 raw value");
    };
    assert_eq!(r.max().real(), peak * peak);
    assert!((peak * peak).is_finite());
    let next = f64::from_bits(peak.abs().to_bits() + 1);
    assert!(!(next * next).is_finite(), "{peak}");
    assert_eq!(r.monotonicity(), Monotonicity::NonMonotonic);
    assert_eq!(r.undefined().map(Undefined::raw), Some(Raw::F64(-f64::MAX)));
}

#[test]
fn a_sampled_scan_over_a_bitfield_visits_only_storable_values() {
    // The high 32 bits of a u64: storable values are multiples of 2^32.
    // (x-1e10)^2 is least at the storable value nearest 1e10:
    // 2*2^32 = 8589934592 (1410065408 away) beats 3*2^32 = 12884901888.
    let raw = field(0xFFFF_FFFF_0000_0000, Some(0), false);
    let c = expr("(x-1e10)^2");
    let r = range(&c, raw);
    assert_eq!(r.coverage(), Coverage::Sampled);
    assert_eq!(r.min().raw(), int(8_589_934_592));
    assert_eq!(r.min().real(), 1_410_065_408.0_f64.powi(2));
    let RawRange::Int(ints) = raw else {
        panic!("an integer range");
    };
    let Raw::Int(max_raw) = r.max().raw() else {
        panic!("an integer raw value");
    };
    assert!(ints.contains(max_raw));
    assert_eq!(max_raw, 0xFFFF_FFFF_0000_0000);
}

#[test]
fn exhaustive_up_to_the_limit_sampled_beyond() {
    // 16 bits: 65536 values, the limit itself.
    assert_eq!(EXHAUSTIVE_LIMIT, 65536);
    let r = range(&expr("x"), field(0xFFFF, None, false));
    assert_eq!(r.coverage(), Coverage::Exact);
    let r = range(&expr("x"), Storage::U16.raw_range());
    assert_eq!(r.coverage(), Coverage::Exact);
    // 17 bits: 131072 values.
    let r = range(&expr("x"), field(0x1_FFFF, None, false));
    assert_eq!(r.coverage(), Coverage::Sampled);
    assert_eq!(ends(&r), (0.0, int(0), 131_071.0, int(131_071)));
}

#[test]
fn the_exhaustive_scan_holds_to_its_limit() {
    // 2^16 values: scanned one by one.
    let ok = exhaustive(Domain::new(field(0xFFFF, None, false)), &Ok);
    assert_eq!(ok.map(|r| r.coverage()), Ok(Coverage::Exact));
}

#[test]
#[cfg(debug_assertions)]
#[should_panic(expected = "exhaustive scan of raw indices 0..=131071")]
fn the_exhaustive_scan_refuses_more_than_its_limit_at_once() {
    // 17 bits: `real_range` samples such a domain, and a wrong dispatch to
    // the one-by-one scan fails here before evaluating anything, rather
    // than running 2^32 evaluations for a 32-bit range (T7: caught, not a
    // TIMEOUT).
    let raw = field(0x1_FFFF, None, false);
    let _ = exhaustive(Domain::new(raw), &|_| panic!("evaluated"));
}

#[test]
fn holes_in_an_integer_range_are_never_visited() {
    // Storable {0, 2, 8, 10}: (x-5)^2 = 25, 9, 9, 25.
    let raw = field(0b1010, Some(0), false);
    let r = range(&expr("(x-5)^2"), raw);
    assert_eq!(ends(&r), (9.0, int(2), 25.0, int(0)));
    let r = range(&expr("x"), raw);
    assert_eq!(ends(&r), (0.0, int(0), 10.0, int(10)));
    let r = range(&linear(1.0, 0.0), raw);
    assert_eq!(ends(&r), (0.0, int(0), 10.0, int(10)));

    // Signed multiples of 4 from -32 to 28: (x-1)^2 is least at 0.
    let raw = field(0xF0, Some(2), true);
    let r = range(&expr("(x-1)^2"), raw);
    assert_eq!(ends(&r), (1.0, int(0), 1089.0, int(-32)));
}

// ---------------------------------------------------------------------------
// The sampled-scan building blocks
// ---------------------------------------------------------------------------

#[test]
fn the_sample_is_a_grid_plus_landmarks_near_zero() {
    // u32: the grid steps (2^32 - 1) / 2^14, about 262144; k * that
    // rounds down, so the grid holds 2^p - 1, never a power of two.
    let d = Domain::new(Storage::U32.raw_range());
    let s = sample(d);
    assert!(s.windows(2).all(|w| w[0] < w[1]));
    assert_eq!((s.first(), s.last()), (Some(&0), Some(&4_294_967_295)));
    for grid in [262_143, 524_287, 2_147_483_647, 4_294_705_151] {
        assert!(s.contains(&grid), "{grid}");
    }
    for landmark in [
        1,
        2,
        3,
        4,
        6,
        8,
        12,
        16,
        24,
        1 << 20,
        3 << 20,
        1 << 31,
        3 << 30,
    ] {
        assert!(s.contains(&landmark), "{landmark}");
    }
    // 16385 grid points; landmarks 2^0..2^31 (32) and 1.5 * 2^1..2^31 (31)
    // beyond 0, none on the grid.
    assert_eq!(s.len(), 16_385 + 32 + 31);

    // i32: the negative landmarks too.
    let s = sample(Domain::new(Storage::I32.raw_range()));
    for landmark in [-1, -2, -3, -4, -6, -8, -12, -(1 << 30), -(3 << 29), 3, 12] {
        assert!(s.contains(&landmark), "{landmark}");
    }

    // A float range is the grid alone (it is already geometric).
    let s = sample(Domain::new(Storage::F32.raw_range()));
    assert_eq!(s.len(), 16_385);
}

#[test]
fn candidates_are_local_extremes_most_extreme_first() {
    let ok = |i: i128, v: f64| (i, Ok(v));
    let gap = |i: i128| (i, Err(ToRealError::NonFinite(f64::INFINITY)));
    // 5 3 4 1 2 _ 6 0
    let samples = [
        ok(0, 5.0),
        ok(10, 3.0),
        ok(20, 4.0),
        ok(30, 1.0),
        ok(40, 2.0),
        gap(50),
        ok(60, 6.0),
        ok(70, 0.0),
    ];
    let got = candidates(&samples);
    assert_eq!(
        got,
        [
            // Minima, lowest first: 0 at 70, 1 at 30, 3 at 10 (2 at 40 is
            // beside the gap, but above 1 on its other side).
            (60, 70, Sense::Min),
            (20, 40, Sense::Min),
            (0, 20, Sense::Min),
            // Maxima, highest first: 6 at 60 (beside the gap), 5 at 0,
            // 4 at 20, 2 at 40 (beside the gap).
            (50, 70, Sense::Max),
            (0, 10, Sense::Max),
            (10, 30, Sense::Max),
            (30, 50, Sense::Max),
        ]
    );
}

#[test]
fn equal_neighbours_are_both_minima_and_maxima() {
    let samples = [(0, Ok(1.0)), (10, Ok(1.0))];
    assert_eq!(
        candidates(&samples),
        [
            (0, 10, Sense::Min),
            (0, 10, Sense::Min),
            (0, 10, Sense::Max),
            (0, 10, Sense::Max)
        ]
    );
}

#[test]
fn candidates_keep_only_the_most_extreme_few() {
    // A zigzag 0 1 0 1 ...: every sample is a local extreme.
    let samples: Vec<(i128, Result<f64, ToRealError>)> = (0..100)
        .map(|i| (i, Ok(f64::from(i32::from(i % 2 == 1)))))
        .collect();
    let got = candidates(&samples);
    assert_eq!(got.len(), 2 * REFINE);
    // Ties stay in raw order: the first eight zeros and the first eight ones.
    let mins: Vec<i128> = got
        .iter()
        .filter(|c| c.2 == Sense::Min)
        .map(|c| c.0)
        .collect();
    assert_eq!(mins, [0, 1, 3, 5, 7, 9, 11, 13]);
    let maxs: Vec<i128> = got
        .iter()
        .filter(|c| c.2 == Sense::Max)
        .map(|c| c.0)
        .collect();
    assert_eq!(maxs, [0, 2, 4, 6, 8, 10, 12, 14]);
}

#[test]
fn refine_finds_a_unimodal_extreme_and_records_what_it_evaluated() {
    let d = Domain::new(Storage::U32.raw_range());
    let f = |x: f64| Ok((x - 777.0).abs());
    let mut seen = Vec::new();
    refine(d, &f, 0, 100_000, Sense::Min, &mut seen);
    assert!(seen.contains(&777), "{seen:?}");
    // About log_1.5(100000) = 28 rounds of two, then the last three.
    assert!(seen.len() < 70, "{}", seen.len());
    assert!(seen.iter().all(|&i| (0..=100_000).contains(&i)));

    let f = |x: f64| Ok(-(x - 1234.0).abs());
    let mut seen = Vec::new();
    refine(d, &f, 1000, 2000, Sense::Max, &mut seen);
    assert!(seen.contains(&1234), "{seen:?}");

    // A bracket of three or fewer is evaluated whole.
    let mut seen = Vec::new();
    refine(d, &f, 5, 7, Sense::Max, &mut seen);
    assert_eq!(seen, [5, 6, 7]);

    // A value with no real value is the worst for either sense.
    let f = |x: f64| {
        if x < 50.0 {
            Err(ToRealError::NonFinite(f64::NAN))
        } else {
            Ok(x)
        }
    };
    for (sense, want) in [(Sense::Min, 50), (Sense::Max, 99)] {
        let mut seen = Vec::new();
        refine(d, &f, 0, 99, sense, &mut seen);
        assert!(seen.contains(&want), "{sense:?} {seen:?}");
    }

    // A tie between two points with no value keeps the lower part, where
    // this function's values are.
    let f = |x: f64| {
        if x < 10.0 {
            Ok(x)
        } else {
            Err(ToRealError::NonFinite(f64::NAN))
        }
    };
    let mut seen = Vec::new();
    refine(d, &f, 0, 99, Sense::Max, &mut seen);
    assert!(seen.contains(&9), "{seen:?}");
}

#[test]
fn accessors_return_what_was_built() {
    let e = Extreme {
        real: 1.5,
        raw: int(3),
    };
    assert_eq!((e.real(), e.raw()), (1.5, int(3)));
    let u = Undefined {
        raw: int(4),
        error: ToRealError::NonFinite(f64::INFINITY),
    };
    assert_eq!(u.raw(), int(4));
    assert_eq!(u.error(), ToRealError::NonFinite(f64::INFINITY));
    // Conversion::real_range is Form::real_range.
    let c = expr("x/2");
    assert_eq!(
        c.real_range(Storage::U8.raw_range()),
        c.form().real_range(Storage::U8.raw_range())
    );
}

// ---------------------------------------------------------------------------
// The shape a solve needs (US-RR-24-9)
// ---------------------------------------------------------------------------

fn shape(c: &Conversion, raw: RawRange) -> Shape {
    range(c, raw).shape()
}

#[test]
fn the_shape_of_an_exhaustive_scan() {
    let u8r = Storage::U8.raw_range();
    // x*x: rises first at raw 1, never falls, a value everywhere.
    assert_eq!(
        shape(&expr("x*x"), u8r),
        Shape {
            first: int(0),
            last: int(255),
            before: None,
            after: None,
            gap: None,
            rise: Some(int(1)),
            fall: None,
        }
    );
    // (x-128)^2: falls first at raw 1, rises first at raw 129.
    let s = shape(&expr("(x-128)^2"), u8r);
    assert_eq!((s.rise, s.fall), (Some(int(129)), Some(int(1))));
    // A plateau first: x - x%4 rises first at raw 4.
    assert_eq!(shape(&expr("x-x%4"), u8r).rise, Some(int(4)));
    // sqrt(x-10) has no value below 10; sqrt(200-x) none above 200.
    let s = shape(&expr("sqrt(x-10)"), u8r);
    assert_eq!(
        (s.first, s.last, s.before, s.after, s.gap),
        (int(10), int(255), Some(int(9)), None, None)
    );
    let s = shape(&expr("sqrt(200-x)"), u8r);
    assert_eq!(
        (s.first, s.last, s.before, s.after, s.gap),
        (int(0), int(200), None, Some(int(201)), None)
    );
    assert_eq!((s.rise, s.fall), (None, Some(int(1))));
    // Both ends, and a hole between: sqrt((x-10)*(200-x)) is NaN outside
    // 10..=200; 1/(x-100) has none at 100 only.
    let s = shape(&expr("sqrt((x-10)*(200-x))"), u8r);
    assert_eq!(
        (s.first, s.last, s.before, s.after),
        (int(10), int(200), Some(int(9)), Some(int(201)))
    );
    let s = shape(&expr("1/(x-100)+0*sqrt(250-x)"), u8r);
    assert_eq!(
        s.gap,
        Some(Undefined::new(
            int(100),
            ToRealError::NonFinite(f64::INFINITY)
        ))
    );
    // After the gap, the run goes on to 250; 251 is the first with none
    // after the last with one.
    assert_eq!((s.last, s.after), (int(250), Some(int(251))));
    // Two holes: the gap is the lower.
    let s = shape(&expr("x/((x-50)*(x-60))"), u8r);
    assert_eq!(s.gap.map(Undefined::raw), Some(int(50)));
    // A run of holes: before is the highest of them.
    let s = shape(&expr("sqrt(x-10)"), Storage::I8.raw_range());
    assert_eq!(s.before, Some(int(9)));
}

#[test]
fn the_shape_of_a_constant() {
    let s = shape(&expr("x-x+3"), Storage::U8.raw_range());
    assert_eq!((s.rise, s.fall), (None, None));
    assert_eq!((s.first, s.last), (int(0), int(255)));
}

#[test]
fn the_shape_of_a_linear_form() {
    // Examined: the ends of the finite run, and the raw values outside it.
    let u8r = Storage::U8.raw_range();
    assert_eq!(
        shape(&linear(2.0, 1.0), u8r),
        Shape {
            first: int(0),
            last: int(255),
            before: None,
            after: None,
            gap: None,
            rise: Some(int(255)),
            fall: None,
        }
    );
    let s = shape(&linear(-2.0, 1.0), u8r);
    assert_eq!((s.rise, s.fall), (None, Some(int(255))));
    let s = shape(&linear(0.0, 1.0), u8r);
    assert_eq!((s.rise, s.fall), (None, None));
    // 2x over f64 overflows at both ends.
    let f64r = Storage::F64.raw_range();
    let s = shape(&linear(2.0, 0.0), f64r);
    let (Raw::F64(first), Raw::F64(last)) = (s.first, s.last) else {
        panic!("{s:?}")
    };
    assert_eq!((first, last), (-f64::MAX / 2.0, f64::MAX / 2.0));
    assert_eq!(
        s.before,
        Some(Raw::F64(-f64::from_bits((f64::MAX / 2.0).to_bits() + 1)))
    );
    assert_eq!(
        s.after,
        Some(Raw::F64(f64::from_bits((f64::MAX / 2.0).to_bits() + 1)))
    );
}

#[test]
fn the_shape_of_a_sampled_scan() {
    // sqrt(x-10) over u32: the landmarks 8 (no value) and 12 (a value)
    // bracket the start, and the refinement of the sample beside the one
    // with no value (a minimum and a maximum) finds 9 and 10.
    let s = shape(&expr("sqrt(x-10)"), Storage::U32.raw_range());
    assert_eq!((s.before, s.first), (Some(int(9)), int(10)));
    assert_eq!((s.after, s.last), (None, int(4_294_967_295)));
    // A scan that begins with a value has no `before`, however it ends.
    let s = shape(&expr("sqrt(3000000000-x)"), Storage::U32.raw_range());
    assert_eq!(s.before, None);
    assert!(matches!(s.after, Some(Raw::Int(v)) if v > 3_000_000_000));
    let (Raw::Int(last), Some(Raw::Int(after))) = (s.last, s.after) else {
        panic!("{s:?}")
    };
    assert!(last <= 3_000_000_000 && after > 3_000_000_000, "{s:?}");
}

// ---------------------------------------------------------------------------
// Properties
// ---------------------------------------------------------------------------

/// Brute force over every storable value: `(min, max, rises, falls)` of the
/// values that exist, in raw order.
fn brute(c: &Conversion, raw: IntRange) -> Option<(f64, f64, bool, bool)> {
    let values: Vec<f64> = (raw.min()..=raw.max())
        .filter(|&v| raw.contains(v))
        .filter_map(|v| c.to_real_raw(Raw::Int(v)).ok())
        .collect();
    let min = values.iter().copied().reduce(f64::min)?;
    let max = values.iter().copied().reduce(f64::max)?;
    let rises = values.windows(2).any(|w| w[1] > w[0]);
    let falls = values.windows(2).any(|w| w[1] < w[0]);
    Some((min, max, rises, falls))
}

fn monotonicity_of(rises: bool, falls: bool) -> Monotonicity {
    match (rises, falls) {
        (false, false) => Monotonicity::Constant,
        (true, false) => Monotonicity::Increasing,
        (false, true) => Monotonicity::Decreasing,
        (true, true) => Monotonicity::NonMonotonic,
    }
}

fn small_int_storage() -> impl Strategy<Value = Storage> {
    prop_oneof![Just(Storage::U8), Just(Storage::I8), Just(Storage::I16)]
}

fn wide_storage() -> impl Strategy<Value = Storage> {
    prop_oneof![
        Just(Storage::U32),
        Just(Storage::I32),
        Just(Storage::U24),
        Just(Storage::I64),
        Just(Storage::F32)
    ]
}

/// Checks `r` against a brute-force scan, and that each extreme is attained
/// where it says.
fn check_exact(c: &Conversion, storage: Storage, r: &RealRange) {
    let ints = storage.raw_range().as_int().unwrap();
    let (min, max, rises, falls) = brute(c, ints).unwrap();
    assert_eq!((r.min().real(), r.max().real()), (min, max));
    assert_eq!(r.monotonicity(), monotonicity_of(rises, falls));
    assert_eq!(at(c, r.min().raw()), min);
    assert_eq!(at(c, r.max().raw()), max);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]

    /// An exhaustive formula range is the brute-force range.
    #[test]
    fn exhaustive_formula_ranges_match_brute_force(
        a in -4i32..=4, h in -300i32..=300, k in -1000i32..=1000, storage in small_int_storage(),
    ) {
        let c = expr(&format!("{a}*(x-{h})^2/8+{k}"));
        let r = range(&c, storage.raw_range());
        prop_assert_eq!(r.coverage(), Coverage::Exact);
        check_exact(&c, storage, &r);
    }

    /// The linear form's analytic range is the brute-force range.
    #[test]
    fn linear_ranges_match_brute_force(
        m in -100.0f64..100.0, b in -1.0e4f64..1.0e4, zero in any::<bool>(),
        storage in small_int_storage(),
    ) {
        let m = if zero { 0.0 } else { m };
        let c = linear(m, b);
        let r = range(&c, storage.raw_range());
        prop_assert_eq!(r.coverage(), Coverage::Exact);
        prop_assert_eq!(r.undefined(), None);
        check_exact(&c, storage, &r);
    }

    /// A lookup table's analytic range is the brute-force range, holes
    /// included.
    #[test]
    fn lut_ranges_match_brute_force(
        knots in proptest::collection::vec((0.5f64..60.0, -50i32..=50), 2..7),
        start in -200.0f64..200.0,
        storage in small_int_storage(),
    ) {
        // Raw values strictly increase; real values are integers, so a
        // value between two points never rounds past either.
        let mut raw = start;
        let points: Vec<(f64, f64)> = knots
            .iter()
            .map(|&(step, y)| {
                raw += step;
                (raw, f64::from(y))
            })
            .collect();
        let c = lut(&points);
        let ints = storage.raw_range().as_int().unwrap();
        match (c.real_range(storage.raw_range()), brute(&c, ints)) {
            (Ok(r), Some(_)) => {
                prop_assert_eq!(r.coverage(), Coverage::Exact);
                check_exact(&c, storage, &r);
                let first_gap = (ints.min()..=ints.max())
                    .find(|&v| c.to_real_raw(Raw::Int(v)).is_err())
                    .map(Raw::Int);
                prop_assert_eq!(r.undefined().map(Undefined::raw), first_gap);
            }
            (Err(RangeError::NoValue { lowest, .. }), None) => {
                prop_assert_eq!(lowest.raw(), Raw::Int(ints.min()));
            }
            (got, want) => prop_assert!(false, "{:?} vs {:?}", got, want),
        }
    }

    /// Over a wide range a unimodal formula's sampled range is still exact:
    /// no probe lies outside it, and each extreme is attained.
    #[test]
    fn sampled_unimodal_ranges_contain_every_probe(
        a in prop_oneof![Just(-1.0f64), Just(1.0)],
        h in -3.0e9f64..3.0e9,
        storage in wide_storage(),
        probes in proptest::collection::vec(any::<u64>(), 16),
    ) {
        let c = expr(&format!("{a}*(x-({h}))^2"));
        let raw = storage.raw_range();
        let r = range(&c, raw);
        prop_assert_eq!(at(&c, r.min().raw()), r.min().real());
        prop_assert_eq!(at(&c, r.max().raw()), r.max().real());
        let d = Domain::new(raw);
        for p in probes {
            let i = d.lo() + (i128::from(p) % (d.hi() - d.lo() + 1));
            let v = at(&c, d.raw(i));
            prop_assert!(r.min().real() <= v && v <= r.max().real(), "{:?} at {:?}", v, d.raw(i));
        }
    }

}

proptest! {
    // Each case runs every range twice; the sampled ones dominate.
    #![proptest_config(ProptestConfig::with_cases(8))]

    /// The same conversion and range always give the same answer, whatever
    /// was asked before and in whatever order (no cache, MF-05).
    #[test]
    fn ranges_do_not_depend_on_call_order(
        order in Just((0..CASES.len() + 2).collect::<Vec<_>>()).prop_shuffle(),
    ) {
        let cases = cases();
        let first: Vec<String> = cases.iter().map(|(c, raw)| format!("{:?}", c.real_range(*raw))).collect();
        for &i in &order {
            let (c, raw) = &cases[i];
            prop_assert_eq!(&format!("{:?}", c.real_range(*raw)), &first[i]);
        }
    }
}

const CASES: [(&str, Storage); 10] = [
    ("-0.5*x+100", Storage::U8),
    ("(x-128)^2", Storage::U8),
    ("1/(x-10)", Storage::U8),
    ("1/(x-10)", Storage::U32),
    ("sqrt(x-10)", Storage::I16),
    ("(x-1000000)^2", Storage::U32),
    ("x*x", Storage::F32),
    ("sin(x/1000)", Storage::I32),
    ("1/(x-x)", Storage::U8),
    ("x*0.01", Storage::U16),
];

fn cases() -> Vec<(Conversion, RawRange)> {
    let mut all: Vec<(Conversion, RawRange)> = CASES
        .iter()
        .map(|&(text, storage)| (expr(text), storage.raw_range()))
        .collect();
    all.push((linear(-0.5, 100.0), Storage::F64.raw_range()));
    all.push((
        lut(&[(0.0, 100.0), (100.0, 50.0), (200.0, 0.0)]),
        Storage::U8.raw_range(),
    ));
    all
}
