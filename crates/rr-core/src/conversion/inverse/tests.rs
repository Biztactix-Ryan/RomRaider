//! Real → raw for linear conversions (US-RR-24-8): known answers worked
//! by hand, the refusals, the tie and range rules, the `to_raw` mismatch
//! check, and the round-trip property over random linear formulas and
//! encodings.

use super::*;
use crate::conversion::{ConversionSpec, FormSpec, Lut};
use crate::diag::{Provenance, Severity, Span};
use crate::encoding::{BitField, Storage};
use proptest::prelude::*;

fn conv(to_real: &str, to_raw: Option<&str>) -> Conversion {
    Conversion::from_spec(&ConversionSpec {
        form: FormSpec::Expr { to_real, to_raw },
        unit: None,
        format: None,
    })
    .unwrap()
}

fn lin(m: f64, b: f64) -> Conversion {
    Conversion::new(Form::Linear(Linear::new(m, b).unwrap()), None, None)
}

fn u8r() -> RawRange {
    Storage::U8.raw_range()
}

fn int(c: &Conversion, real: f64, raw: RawRange) -> Result<i128, Reject> {
    c.to_raw_checked(real, raw).map(|r| match r {
        Raw::Int(v) => v,
        other => panic!("expected an integer, got {other:?}"),
    })
}

fn expr_form(c: &Conversion) -> &ExprForm {
    let Form::Expr(e) = c.form() else {
        panic!("expected an Expr form")
    };
    e
}

// ---------------------------------------------------------------------------
// Known answers
// ---------------------------------------------------------------------------

#[test]
fn increasing_linear_formula_inverts_to_the_nearest_raw_value() {
    let c = conv("x*0.5", None);
    // 10 / 0.5 = 20.
    assert_eq!(int(&c, 10.0, u8r()), Ok(20));
    // 10.1: raw 20 is 10.0 (0.1 away), raw 21 is 10.5 (0.4 away).
    assert_eq!(int(&c, 10.1, u8r()), Ok(20));
    // 10.4: raw 21 (0.1 away).
    assert_eq!(int(&c, 10.4, u8r()), Ok(21));
    // 10.25 is exactly between 10.0 and 10.5: the larger raw value, as
    // legacy's Math.round(10.25 / 0.5 = 20.5) = 21.
    assert_eq!(int(&c, 10.25, u8r()), Ok(21));
    // The ends: 0 and 255 * 0.5 = 127.5.
    assert_eq!(int(&c, 0.0, u8r()), Ok(0));
    assert_eq!(int(&c, 127.5, u8r()), Ok(255));
}

#[test]
fn decreasing_linear_formula_inverts_the_right_way_round() {
    // 100 - x/2: raw 0 is 100, raw 255 is -27.5.
    let c = conv("100-x/2", None);
    assert_eq!(int(&c, 100.0, u8r()), Ok(0));
    assert_eq!(int(&c, -27.5, u8r()), Ok(255));
    // 60 = 100 - x/2 at x = 80.
    assert_eq!(int(&c, 60.0, u8r()), Ok(80));
    // 50.25: between raw 99 (50.5) and raw 100 (50.0), exactly halfway:
    // the larger raw value, as legacy's Math.round((100 - 50.25) * 2 =
    // 99.5) = 100.
    assert_eq!(int(&c, 50.25, u8r()), Ok(100));
    // 50.3 is nearer 50.5 (raw 99).
    assert_eq!(int(&c, 50.3, u8r()), Ok(99));
}

#[test]
fn ties_are_decided_on_the_exact_values_not_rounded_differences() {
    // x*0.1: raw 1 is 0.1000000000000000055..., raw 2 is
    // 0.2000000000000000111...; 0.15 is 0.1499999999999999944...: exactly
    // 0.0499999999999999888... from raw 1 and 0.0500000000000000166...
    // from raw 2, so raw 1 (legacy: Math.round(0.15/0.1 =
    // 1.4999999999999998) = 1, the same).
    let c = conv("x*0.1", None);
    assert_eq!(int(&c, 0.15, u8r()), Ok(1));
    // 0.25 is 0.0499999999999999888... from raw 2 (0.2000000000000000111)
    // and 0.0500000000000000444... from raw 3 (0.3000000000000000444):
    // raw 2, the nearer. (Legacy's frexpr x/0.1 gives 2.5 after rounding,
    // and Math.round makes that 3: one raw step apart, both 0.05 away in
    // decimal; here the nearer binary value wins.)
    assert_eq!(int(&c, 0.25, u8r()), Ok(2));
}

#[test]
fn a_target_beyond_half_a_step_past_the_range_is_refused_never_clamped() {
    let c = conv("x*0.5", None);
    // 127.6 is 0.1 above 127.5 (raw 255), within half a step (0.25):
    // raw 255.
    assert_eq!(int(&c, 127.6, u8r()), Ok(255));
    // 127.75 is exactly half a step above: its nearest raw value on the
    // grid is 256 (half up), which the cell cannot hold.
    let err = c.to_raw_checked(127.75, u8r()).unwrap_err();
    assert_eq!(
        err,
        Reject::OutOfRange {
            target: 127.75,
            min: 0.0,
            max: 127.5,
            nearest: Raw::Int(255)
        }
    );
    assert_eq!(
        err.to_string(),
        "127.75 is outside what the cell can hold, 0 to 127.5; the nearest end is raw value 255"
    );
    assert_eq!(err.code(), codes::EDIT_OUT_OF_RANGE);
    // Far above.
    assert!(matches!(
        c.to_raw_checked(1000.0, u8r()),
        Err(Reject::OutOfRange { max: 127.5, .. })
    ));
    // Below: exactly half a step below raw 0 rounds up to 0, accepted;
    // more than half is refused.
    assert_eq!(int(&c, -0.25, u8r()), Ok(0));
    assert_eq!(
        c.to_raw_checked(-0.26, u8r()),
        Err(Reject::OutOfRange {
            target: -0.26,
            min: 0.0,
            max: 127.5,
            nearest: Raw::Int(0)
        })
    );
    assert_eq!(
        c.to_raw_checked(-0.26, u8r()).unwrap_err().to_string(),
        "-0.26 is outside what the cell can hold, 0 to 127.5; the nearest end is raw value 0"
    );
    assert!(c.to_raw_checked(-1000.0, u8r()).is_err());
}

#[test]
fn a_decreasing_range_is_refused_at_the_right_ends() {
    // 100 - x/2 over u8: 100 (raw 0) down to -27.5 (raw 255).
    let c = conv("100-x/2", None);
    // Half a step past raw 255 (-27.75) rounds to 256: refused.
    assert_eq!(
        c.to_raw_checked(-27.75, u8r()),
        Err(Reject::OutOfRange {
            target: -27.75,
            min: -27.5,
            max: 100.0,
            nearest: Raw::Int(255)
        })
    );
    assert_eq!(int(&c, -27.7, u8r()), Ok(255));
    // Half a step past raw 0 (100.25) rounds to raw 0: accepted.
    assert_eq!(int(&c, 100.25, u8r()), Ok(0));
    assert!(c.to_raw_checked(100.26, u8r()).is_err());
}

#[test]
fn signed_storage_inverts_negative_raw_values() {
    let c = conv("x*0.25", None);
    let i8r = Storage::I8.raw_range();
    assert_eq!(int(&c, -32.0, i8r), Ok(-128));
    assert_eq!(int(&c, 31.75, i8r), Ok(127));
    assert_eq!(int(&c, -0.25, i8r), Ok(-1));
    // -32.125 is half a step below raw -128: accepted (half up is -128).
    assert_eq!(int(&c, -32.125, i8r), Ok(-128));
    assert!(c.to_raw_checked(-32.13, i8r).is_err());
    // 31.875 is half a step above raw 127: refused.
    assert!(c.to_raw_checked(31.875, i8r).is_err());
}

#[test]
fn four_and_eight_byte_storage_never_overflows() {
    // MF-05: legacy cast the result to a Java int, so a u32 above 2^31 - 1
    // saturated. 3_000_000_000 * 0.001 = 3000000 (to the f64 nearest).
    let c = conv("x*0.001", None);
    let u32r = Storage::U32.raw_range();
    for r in [
        0_i128,
        1,
        2_147_483_647,
        2_147_483_648,
        3_000_000_000,
        4_294_967_295,
    ] {
        let real = c.to_real(r as f64).unwrap();
        assert_eq!(int(&c, real, u32r), Ok(r), "raw {r}");
    }
    let i32r = Storage::I32.raw_range();
    for r in [-2_147_483_648_i128, -1, 0, 2_147_483_647] {
        let real = c.to_real(r as f64).unwrap();
        assert_eq!(int(&c, real, i32r), Ok(r), "raw {r}");
    }
    // A step past u32::MAX is refused, not wrapped to 0.
    assert!(c.to_raw_checked(4_294_967.296, u32r).is_err());
    // 64-bit: the largest values. u64::MAX is 2^64 - 1, which as an f64
    // is 2^64, as is u64::MAX - 1: equally near, the larger wins.
    let x = conv("x", None);
    let u64r = Storage::U64.raw_range();
    assert_eq!(
        int(&x, 18_446_744_073_709_551_615.0, u64r),
        Ok(u64::MAX.into())
    );
    let i64r = Storage::I64.raw_range();
    // -2^63 is exact; i64::MIN + 1 rounds to it too, so the larger of the
    // two nearest the estimate (the window is one below to two above).
    let got = int(&x, -9_223_372_036_854_775_808.0, i64r).unwrap();
    assert_eq!(got as f64, -9_223_372_036_854_775_808.0);
    assert_eq!(int(&x, 0.0, i64r), Ok(0));
    assert!(x.to_raw_checked(1e19, i64r).is_err());
}

#[test]
fn a_bitfield_with_holes_inverts_to_a_storable_value() {
    // Mask 0xF0 with no shift: 0, 16, ..., 240.
    let raw = RawRange::Int(BitField::new(0xF0, Some(0), false).unwrap().raw_range());
    let c = conv("x", None);
    assert_eq!(int(&c, 16.0, raw), Ok(16));
    // 20 is nearer 16 than 32.
    assert_eq!(int(&c, 20.0, raw), Ok(16));
    // 24 is halfway: the larger.
    assert_eq!(int(&c, 24.0, raw), Ok(32));
    // 247 is within half a step (8) of 240; 248 is exactly half.
    assert_eq!(int(&c, 247.0, raw), Ok(240));
    assert!(c.to_raw_checked(248.0, raw).is_err());
    // -8 is half a step below 0: accepted.
    assert_eq!(int(&c, -8.0, raw), Ok(0));
    assert!(c.to_raw_checked(-8.5, raw).is_err());
}

#[test]
fn float_storage_inverts_to_the_nearest_float() {
    let c = conv("x*2", None);
    let f32r = Storage::F32.raw_range();
    assert_eq!(c.to_raw_checked(3.0, f32r), Ok(Raw::F32(1.5)));
    // 0.1f32 * 2 in f64 comes back to 0.1f32.
    let real = c.to_real(f64::from(0.1_f32)).unwrap();
    assert_eq!(c.to_raw_checked(real, f32r), Ok(Raw::F32(0.1)));
    // Past f32::MAX: x itself, beyond half an ulp (2^103) of f32::MAX.
    let x = conv("x", None);
    let max = f64::from(f32::MAX);
    assert_eq!(
        x.to_raw_checked(max + 2.0_f64.powi(102), f32r),
        Ok(Raw::F32(f32::MAX))
    );
    assert!(matches!(
        x.to_raw_checked(max + 2.0_f64.powi(103), f32r),
        Err(Reject::OutOfRange { .. })
    ));
    assert_eq!(
        x.to_raw_checked(-max - 2.0_f64.powi(102), f32r),
        Ok(Raw::F32(-f32::MAX))
    );
    let f64r = Storage::F64.raw_range();
    assert_eq!(c.to_raw_checked(1e300, f64r), Ok(Raw::F64(5e299)));
    assert_eq!(x.to_raw_checked(-0.0, f64r), Ok(Raw::F64(0.0)));
}

#[test]
fn a_target_past_where_the_conversion_overflows_is_refused() {
    // 2x over f64 is finite up to x = f64::MAX / 2, which gives f64::MAX.
    let c = lin(2.0, 0.0);
    let f64r = Storage::F64.raw_range();
    assert_eq!(
        c.to_raw_checked(f64::MAX, f64r),
        Ok(Raw::F64(f64::MAX / 2.0))
    );
    assert_eq!(
        c.to_raw_checked(-f64::MAX, f64r),
        Ok(Raw::F64(-f64::MAX / 2.0))
    );
    // 1e308 * x - 1e308 is finite only while 1e308 * x is (x < ~1.797):
    // the largest real value is about 0.797e308, so 1e308 has no raw value
    // and must not be "rounded" to that one.
    let c = lin(1e308, -1e308);
    let err = c.to_raw_checked(1e308, f64r).unwrap_err();
    let Reject::NonFinite(NonFinite::Beyond {
        target: real,
        limit,
        raw: Raw::F64(raw),
        beyond,
    }) = err
    else {
        panic!("{err:?}")
    };
    assert_eq!(real, 1e308);
    assert!(limit < 1e308 && limit > 0.79e308, "{limit}");
    // The next float up from the last raw value with a value has none.
    let Raw::F64(next) = beyond.raw() else {
        panic!("{beyond:?}")
    };
    assert_eq!(next, f64::from_bits(raw.to_bits() + 1));
    assert!(matches!(beyond.error(), ToRealError::NonFinite(v) if v == f64::INFINITY));
    assert_eq!(err.code(), codes::EDIT_NOT_FINITE);
    assert!(
        err.to_string()
            .contains("is beyond the last real value the conversion gives on that side"),
        "{err}"
    );
    assert_eq!(c.to_raw_checked(0.0, f64r), Ok(Raw::F64(1.0)));
}

#[test]
fn linear_forms_invert_as_formulas_do() {
    let c = lin(0.5, -10.0);
    assert_eq!(int(&c, 0.0, u8r()), Ok(20));
    assert_eq!(int(&c, 117.5, u8r()), Ok(255));
    assert!(c.to_raw_checked(117.75, u8r()).is_err());
    let d = lin(-0.5, 100.0);
    assert_eq!(int(&d, -27.5, u8r()), Ok(255));
}

#[test]
fn a_given_to_raw_formula_is_not_used() {
    // The to_raw formula is wrong (x*0.5 again); the exact inverse of
    // to_real is used anyway.
    let c = conv("x*0.5", Some("x*0.5"));
    assert_eq!(int(&c, 10.0, u8r()), Ok(20));
}

// ---------------------------------------------------------------------------
// Refusals
// ---------------------------------------------------------------------------

#[test]
fn refusals_carry_their_reason() {
    let c = conv("x*0.5", None);
    for v in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let err = c.to_raw_checked(v, u8r()).unwrap_err();
        assert!(
            matches!(err, Reject::NonFinite(NonFinite::Target(e)) if e.to_bits() == v.to_bits())
        );
    }
    assert_eq!(
        c.to_raw_checked(f64::NAN, u8r()).unwrap_err().to_string(),
        "NaN is not a finite number"
    );
    // A constant: no raw value is the one for any value, not even 5.
    let k = conv("x*0+5", None);
    assert_eq!(
        k.to_raw_checked(5.0, u8r()),
        Err(Reject::Singular { real: 5.0 })
    );
    assert_eq!(
        k.to_raw_checked(5.0, u8r()).unwrap_err().to_string(),
        "every raw value converts to 5, so no raw value can be chosen for another"
    );
    assert_eq!(
        lin(0.0, 1.0).to_raw_checked(1.0, u8r()),
        Err(Reject::Singular { real: 1.0 })
    );
    // Not linear, but monotonic over u8: solved (US-RR-24-9), and a
    // value that is no number is refused all the same.
    let sq = conv("x*x", Some("sqrt(x)"));
    assert_eq!(int(&sq, 4.0, u8r()), Ok(2));
    assert!(
        matches!(sq.to_raw_checked(f64::NAN, u8r()), Err(Reject::NonFinite(NonFinite::Target(v))) if v.is_nan())
    );
    let table = Conversion::new(
        Form::Lut(Lut::new(&[(0.0, 0.0), (10.0, 5.0)]).unwrap()),
        None,
        None,
    );
    assert_eq!(int(&table, 1.0, u8r()), Ok(2));
    assert_eq!(table.form().linear(), None);
    // Every refusal carries a code: the conversion's (singular, not
    // invertible) and a value's (not finite, out of range).
    assert_eq!(Reject::Singular { real: 1.0 }.code(), codes::NOT_INVERTIBLE);
    assert_eq!(
        Reject::NonFinite(NonFinite::Target(f64::NAN)).code(),
        codes::EDIT_NOT_FINITE
    );
    assert_eq!(
        Reject::NonFinite(NonFinite::NoValue { target: 1.0 }).code(),
        codes::EDIT_NOT_FINITE
    );
    assert_eq!(
        Reject::NonFinite(NonFinite::NoValue { target: 2.0 }).to_string(),
        "no raw value near 2 has a finite real value, so none can be chosen"
    );
}

#[test]
fn form_linear_reports_the_coefficients() {
    assert_eq!(lin(0.5, 3.0).form().linear(), Linear::new(0.5, 3.0).ok());
    assert_eq!(
        conv("(x-40)/2", None).form().linear(),
        Linear::new(0.5, -20.0).ok()
    );
    assert_eq!(conv("x*x", None).form().linear(), None);
}

// ---------------------------------------------------------------------------
// The solver itself
// ---------------------------------------------------------------------------

#[test]
fn a_wrong_estimate_falls_back_to_bisection_with_the_same_answer() {
    let d = Domain::new(u8r());
    let f = |x: f64| Ok(x * 0.5);
    // A good estimate: no bisection.
    assert_eq!(
        solve(d, (d.lo(), d.hi()), &f, 10.0, true, Some(20.0)),
        Ok(Solved {
            index: 20,
            bisected: false
        })
    );
    // An estimate far too high, or too low: the window does not bracket
    // the target, so it is found by bisection.
    for estimate in [200.0, 3.0, -50.0, 1e9] {
        assert_eq!(
            solve(d, (d.lo(), d.hi()), &f, 10.0, true, Some(estimate)),
            Ok(Solved {
                index: 20,
                bisected: true
            }),
            "estimate {estimate}"
        );
    }
    // Decreasing: 100 - x/2 at 60 is raw 80.
    let g = |x: f64| Ok(100.0 - x * 0.5);
    assert_eq!(
        solve(d, (d.lo(), d.hi()), &g, 60.0, false, Some(80.0)).map(|s| s.bisected),
        Ok(false)
    );
    for estimate in [0.0, 200.0, 77.0, 84.0] {
        assert_eq!(
            solve(d, (d.lo(), d.hi()), &g, 60.0, false, Some(estimate)),
            Ok(Solved {
                index: 80,
                bisected: true
            }),
            "estimate {estimate}"
        );
    }
    // The window is one below to two above the estimate's floor: an
    // estimate of 78.x or 81 brackets 80 without bisecting.
    assert_eq!(
        solve(d, (d.lo(), d.hi()), &g, 60.0, false, Some(78.5)).map(|s| s.bisected),
        Ok(false)
    );
    assert_eq!(
        solve(d, (d.lo(), d.hi()), &g, 60.0, false, Some(81.0)).map(|s| s.bisected),
        Ok(false)
    );
    assert_eq!(
        solve(d, (d.lo(), d.hi()), &g, 60.0, false, Some(82.0)).map(|s| s.bisected),
        Ok(true)
    );
    assert_eq!(
        solve(d, (d.lo(), d.hi()), &g, 60.0, false, Some(77.5)).map(|s| s.bisected),
        Ok(true)
    );
    // At the ends the window is open on that side: no bisection.
    assert_eq!(
        solve(d, (d.lo(), d.hi()), &f, 0.0, true, Some(-3.0)).map(|s| s.bisected),
        Ok(false)
    );
    assert_eq!(
        solve(d, (d.lo(), d.hi()), &f, 127.5, true, Some(1e9)).map(|s| s.bisected),
        Ok(false)
    );
}

#[test]
fn the_estimate_is_the_algebraic_inverse() {
    // (0 - -10) / 0.5 = 20; (60 - 100) / -0.5 = 80; (7 - 3) / 2 = 2.
    assert_eq!(estimate(Linear::new(0.5, -10.0).unwrap(), 0.0), 20.0);
    assert_eq!(estimate(Linear::new(-0.5, 100.0).unwrap(), 60.0), 80.0);
    assert_eq!(estimate(Linear::new(2.0, 3.0).unwrap(), 7.0), 2.0);
}

#[test]
fn a_bisected_bracket_keeps_the_value_below() {
    let d = Domain::new(u8r());
    // 10.1 is between raw 20 (10.0, nearer) and 21 (10.5).
    let f = |x: f64| Ok(x * 0.5);
    assert_eq!(
        solve(d, (d.lo(), d.hi()), &f, 10.1, true, Some(200.0)),
        Ok(Solved {
            index: 20,
            bisected: true
        })
    );
    // 60.3 is between raw 79 (60.5, nearer) and 80 (60.0).
    let g = |x: f64| Ok(100.0 - x * 0.5);
    assert_eq!(
        solve(d, (d.lo(), d.hi()), &g, 60.3, false, Some(0.0)),
        Ok(Solved {
            index: 79,
            bisected: true
        })
    );
    // At the ends of the range the bracket is clamped into it.
    assert_eq!(
        solve(d, (d.lo(), d.hi()), &f, 0.1, true, Some(200.0)).map(|s| s.index),
        Ok(0)
    );
    assert_eq!(
        solve(d, (d.lo(), d.hi()), &f, 127.4, true, Some(3.0)).map(|s| s.index),
        Ok(255)
    );
}

#[test]
fn the_solver_skips_raw_values_with_no_real_value() {
    let d = Domain::new(u8r());
    // No value anywhere.
    let none = |_: f64| Err(ToRealError::NonFinite(f64::NAN));
    assert_eq!(
        solve(d, (d.lo(), d.hi()), &none, 1.0, true, Some(1.0)),
        Err(Reject::NonFinite(NonFinite::NoValue { target: 1.0 }))
    );
    // Overflow above 100 (an increasing formula): 100 itself is the
    // largest value, 50; 50.2 is within half a step (0.25) of it.
    let capped = |x: f64| {
        if x > 100.0 {
            Err(ToRealError::NonFinite(f64::INFINITY))
        } else {
            Ok(x * 0.5)
        }
    };
    assert_eq!(
        solve(d, (d.lo(), d.hi()), &capped, 50.2, true, Some(100.4)).map(|s| s.index),
        Ok(100)
    );
    // Past it: no value beyond, not out of the cell's range.
    assert_eq!(
        solve(d, (d.lo(), d.hi()), &capped, 50.25, true, Some(100.5)),
        Err(Reject::NonFinite(NonFinite::Beyond {
            target: 50.25,
            limit: 50.0,
            raw: Raw::Int(100),
            beyond: Undefined::new(Raw::Int(101), ToRealError::NonFinite(f64::INFINITY)),
        }))
    );
    assert!(matches!(
        solve(d, (d.lo(), d.hi()), &capped, 90.0, true, Some(180.0)),
        Err(Reject::NonFinite(NonFinite::Beyond { limit: 50.0, .. }))
    ));
    // Some other failure counts as no value too (NaN compares false).
    let odd = |x: f64| {
        if x > 100.0 {
            Err(ToRealError::OutsideTable {
                raw: x,
                min: 0.0,
                max: 100.0,
            })
        } else {
            Ok(x)
        }
    };
    assert_eq!(
        solve(d, (d.lo(), d.hi()), &odd, 40.0, true, Some(40.0)).map(|s| s.index),
        Ok(40)
    );
    assert!(solve(d, (d.lo(), d.hi()), &odd, 150.0, true, Some(150.0)).is_err());
    // A raw value below with no value: a decreasing formula undefined
    // below 10, so 10 is the end on that side.
    let low = |x: f64| {
        if x < 10.0 {
            Err(ToRealError::NonFinite(f64::INFINITY))
        } else {
            Ok(100.0 - x)
        }
    };
    assert_eq!(
        solve(d, (d.lo(), d.hi()), &low, 90.4, false, Some(9.6)).map(|s| s.index),
        Ok(10)
    );
    assert_eq!(
        solve(d, (d.lo(), d.hi()), &low, 90.5, false, Some(9.5)).map(|s| s.index),
        Ok(10)
    );
    assert!(solve(d, (d.lo(), d.hi()), &low, 90.6, false, Some(9.4)).is_err());
}

#[test]
fn distances_compare_exactly() {
    let tiny = 2.0_f64.powi(-60);
    // |tiny - 1| and |tiny + 1| are both 1.0 in f64; exactly they differ.
    let near = distance(tiny, 1.0);
    let far = distance(tiny, -1.0);
    assert_eq!((near.hi, far.hi), (1.0, 1.0));
    assert_eq!((near.lo, far.lo), (-tiny, tiny));
    assert!(near.less(far));
    assert!(!far.less(near));
    assert!(!near.less(near));
    // Order of the operands does not matter.
    assert_eq!(distance(1.0, tiny), near);
    assert_eq!(distance(-1.0, tiny), far);
    // Plainly different.
    assert!(distance(0.0, 1.0).less(distance(0.0, 2.0)));
    assert!(!distance(0.0, 2.0).less(distance(0.0, 1.0)));
    // Overflow is infinitely far.
    assert_eq!(
        distance(f64::MAX, -f64::MAX),
        Dist {
            hi: f64::INFINITY,
            lo: 0.0
        }
    );
    // Doubling is exact.
    assert_eq!(
        near.double(),
        Dist {
            hi: 2.0,
            lo: -2.0 * tiny
        }
    );
}

#[test]
fn round_half_up_is_java_math_round() {
    for (v, want) in [
        (0.5, 1.0),
        (-0.5, 0.0),
        (1.4999999999999998, 1.0),
        (-1.5, -1.0),
        (2.5, 3.0),
        (-2.6, -3.0),
        (7.0, 7.0),
        (0.49999999999999994, 0.0),
    ] {
        assert_eq!(round_half_up(v), want, "{v}");
    }
    assert!(round_half_up(f64::NAN).is_nan());
    assert_eq!(round_half_up(f64::INFINITY), f64::INFINITY);
    assert_eq!(round_half_up(f64::NEG_INFINITY), f64::NEG_INFINITY);
}

// ---------------------------------------------------------------------------
// A given to_raw that disagrees
// ---------------------------------------------------------------------------

#[test]
fn a_matching_to_raw_is_no_mismatch() {
    for (to_real, to_raw) in [
        ("x*0.5", "x/0.5"),
        ("x*0.5", "x*2"),
        ("x*0.25-50", "(x+50)/0.25"),
        ("100-x/2", "(100-x)*2"),
        ("x*0.1", "x/0.1"),
    ] {
        let c = conv(to_real, Some(to_raw));
        for raw in [u8r(), Storage::I16.raw_range(), Storage::U32.raw_range()] {
            assert_eq!(
                expr_form(&c).to_raw_mismatch(raw),
                None,
                "{to_real} / {to_raw}"
            );
        }
    }
}

#[test]
fn a_disagreeing_to_raw_is_found_at_its_first_raw_value() {
    // x*0.5 "inverted" by x*0.5: raw 1 is 0.5, which gives 0.25, rounded
    // half up to 0, which displays as 0, not 0.5. Raw 0 agrees.
    let c = conv("x*0.5", Some("x*0.5"));
    let m = expr_form(&c).to_raw_mismatch(u8r()).unwrap();
    assert_eq!((m.raw(), m.real(), m.given()), (Raw::Int(1), 0.5, 0.0));
    assert_eq!(m.code(), codes::TO_RAW_MISMATCH);
    assert_eq!(
        m.to_string(),
        "the to_raw formula does not invert the to_real formula: raw value 1 converts to 0.5, \
         which to_raw turns into 0; edits use the inverse of to_real instead"
    );
    let d = m.to_diagnostic(Provenance::new("defs.xml", Span::line_col(1, 1)));
    assert_eq!(d.severity(), Severity::Warning);
    assert_eq!(d.code().as_str(), "CNV0005");
    // An offset off by one: raw 0 is -40, which gives 1.
    let c = conv("x-40", Some("x+41"));
    let m = expr_form(&c).to_raw_mismatch(u8r()).unwrap();
    assert_eq!((m.raw(), m.given()), (Raw::Int(0), 1.0));
    // A to_raw with no value: NaN is a mismatch.
    let c = conv("x", Some("x/0"));
    let m = expr_form(&c).to_raw_mismatch(u8r()).unwrap();
    assert_eq!(m.raw(), Raw::Int(0));
    assert!(m.given().is_nan());
    // A raw value with no real value is skipped: 1/(x-0) has none at 0.
    let c = conv("100/x", Some("x+1"));
    let m = expr_form(&c).to_raw_mismatch(u8r()).unwrap();
    assert_eq!(m.raw(), Raw::Int(1));
}

#[test]
fn a_to_raw_that_lands_on_a_raw_value_with_the_same_display_is_no_mismatch() {
    // x*0 + floor-ish collisions: raw 2*k and 2*k+1 display the same with
    // to_real x - x%2 (not linear, but the check is general); to_raw x+1
    // turns raw 0's value 0 into 1, which displays as 0 too.
    let c = conv("x-x%2", Some("x+1"));
    assert_eq!(expr_form(&c).to_raw_mismatch(u8r()), None);
}

#[test]
fn the_mismatch_check_is_exhaustive_up_to_the_limit_and_a_grid_beyond() {
    // Disagrees only above 1000.
    let c = conv("x", Some("if(x>1000.5,x+5,x)"));
    let e = expr_form(&c);
    // 2^16 values: every one is checked, so the first is 1001.
    assert_eq!(
        e.to_raw_mismatch(Storage::U16.raw_range()).map(|m| m.raw()),
        Some(Raw::Int(1001))
    );
    // u32: an even grid of 2^16 points, (2^32 - 1) * k / 65535 = 65537 * k
    // (2^32 - 1 = 65535 * 65537); the first above 1000 is k = 1.
    assert_eq!(
        e.to_raw_mismatch(Storage::U32.raw_range()).map(|m| m.raw()),
        Some(Raw::Int(65_537))
    );
    // i32: -2^31 + 65537 * k; the first above 1000 is k = 32768, which is
    // -2147483648 + 2147516416 = 32768.
    assert_eq!(
        e.to_raw_mismatch(Storage::I32.raw_range()).map(|m| m.raw()),
        Some(Raw::Int(32_768))
    );
    // The last grid point is the end of the range: one disagreeing only
    // there is found.
    let c = conv("x", Some("if(x>4294967294.5,0,x)"));
    assert_eq!(
        expr_form(&c)
            .to_raw_mismatch(Storage::U32.raw_range())
            .map(|m| m.raw()),
        Some(Raw::Int(4_294_967_295))
    );
}

#[test]
fn no_to_raw_or_a_float_range_has_no_mismatch() {
    assert_eq!(expr_form(&conv("x*0.5", None)).to_raw_mismatch(u8r()), None);
    let c = conv("x*0.5", Some("x*0.5"));
    assert_eq!(
        expr_form(&c).to_raw_mismatch(Storage::F32.raw_range()),
        None
    );
}

// ---------------------------------------------------------------------------
// Properties
// ---------------------------------------------------------------------------

/// A constant as formula text: positive, read back exactly.
fn lit(v: f64) -> String {
    format!("{v:?}")
}

/// A linear formula of one of the shapes definitions use, and its
/// expected direction (`true` increasing).
fn formula() -> impl Strategy<Value = (String, bool)> {
    let factor = prop_oneof![
        (1_u32..=100_000).prop_map(|n| f64::from(n) / 1000.0),
        (1_u32..=1000).prop_map(f64::from),
        (1_u32..=4096).prop_map(|n| f64::from(n) / 65536.0),
    ];
    let offset = prop_oneof![
        Just(0.0),
        (0_u32..=100_000).prop_map(|n| f64::from(n) / 100.0)
    ];
    (0_u8..6, factor, offset.clone(), offset).prop_map(|(shape, k, b, c)| {
        let (k, b, c) = (lit(k), lit(b), lit(c));
        match shape {
            0 => (format!("x*{k}+{b}"), true),
            1 => (format!("(x+{c})/{k}"), true),
            2 => (format!("{b}-x/{k}"), false),
            3 => (format!("x/{k}*{k}-{b}"), true),
            4 => (format!("{k}*(x/{k}*{k})+{c}"), true),
            _ => (format!("-(x*{k}-{b})"), false),
        }
    })
}

/// A raw range: whole storages, and bitfields (some with holes).
fn raw_range() -> impl Strategy<Value = RawRange> {
    prop_oneof![
        prop::sample::select(vec![
            Storage::U8,
            Storage::I8,
            Storage::U16,
            Storage::I16,
            Storage::U24,
            Storage::I24,
            Storage::U32,
            Storage::I32,
            Storage::U64,
            Storage::I64,
            Storage::F32,
        ])
        .prop_map(Storage::raw_range),
        (1_u64..=0xFFFF, prop::option::of(0_u32..4), any::<bool>()).prop_filter_map(
            "a valid bitfield",
            |(mask, shift, signed)| {
                BitField::new(mask, shift, signed)
                    .ok()
                    .map(|f| RawRange::Int(f.raw_range()))
            }
        ),
    ]
}

/// A raw index: anywhere in the domain, or near its ends or near zero
/// (where `f64` is exact), as a fraction picked from `pick`.
fn index_in(d: Domain, pick: (u8, u64)) -> i128 {
    let (zone, n) = pick;
    let len = d.len();
    let off = i128::try_from(u128::from(n) % len).unwrap_or(0);
    match zone % 4 {
        0 => d.lo() + off,
        1 => d.lo() + i128::try_from(u128::from(n % 64) % len).unwrap_or(0),
        2 => d.hi() - i128::try_from(u128::from(n % 64) % len).unwrap_or(0),
        _ => {
            let zero = d.partition(|i| d.value(i) >= 0.0).min(d.hi());
            (zero + i128::from(n % 4096) - 2048).clamp(d.lo(), d.hi())
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(2000))]

    /// to_raw(to_real(r)) == r for every storable r whose real value no
    /// neighbour shares, over random linear formulas and encodings.
    #[test]
    fn linear_formulas_round_trip_exactly(
        (text, increasing) in formula(),
        raw in raw_range(),
        pick in (any::<u8>(), any::<u64>()),
    ) {
        let c = conv(&text, None);
        let line = c.form().linear();
        prop_assert!(line.is_some(), "{text} not detected");
        prop_assert_eq!(line.unwrap().m() > 0.0, increasing, "{}", text);
        let d = Domain::new(raw);
        let i = index_in(d, pick);
        let r = d.raw(i);
        let Ok(real) = c.to_real_raw(r) else { return Ok(()) };
        let shared = [i - 1, i + 1]
            .into_iter()
            .filter(|j| (d.lo()..=d.hi()).contains(j))
            .any(|j| c.to_real_raw(d.raw(j)) == Ok(real));
        let got = c.to_raw_checked(real, raw);
        if shared {
            // Indistinguishable in f64: some raw value with that display.
            let back = got.map(|g| c.to_real_raw(g));
            prop_assert_eq!(back, Ok(Ok(real)), "{} at {:?}", text, r);
        } else {
            prop_assert_eq!(got, Ok(r), "{} at {:?}", text, r);
        }
    }

    /// For any real value within the range, the raw value chosen is at
    /// least as near as its neighbours.
    #[test]
    fn the_chosen_raw_value_is_the_nearest(
        (text, _) in formula(),
        raw in raw_range(),
        pick in (any::<u8>(), any::<u64>()),
        frac in -0.6_f64..0.6,
    ) {
        let c = conv(&text, None);
        prop_assert!(c.form().linear().is_some(), "{} not detected", text);
        let d = Domain::new(raw);
        let i = index_in(d, pick);
        let Ok(a) = c.to_real_raw(d.raw(i)) else { return Ok(()) };
        let next = (i + 1).min(d.hi());
        let Ok(b) = c.to_real_raw(d.raw(next)) else { return Ok(()) };
        let t = a + (b - a) * frac;
        if let Ok(got) = c.to_raw_checked(t, raw) {
            let g = d.partition(|j| d.raw(j) == got || raw_gt(d.raw(j), got));
            let here = distance(t, c.to_real_raw(got).unwrap());
            for j in [g - 1, g + 1] {
                if (d.lo()..=d.hi()).contains(&j)
                    && let Ok(v) = c.to_real_raw(d.raw(j))
                {
                    prop_assert!(!distance(t, v).less(here), "{} at {}", text, t);
                }
            }
        }
    }

    /// The same question gets the same answer whatever was asked before
    /// and in whatever order: no cache.
    #[test]
    fn answers_do_not_depend_on_call_order(
        (text, _) in formula(),
        raw in raw_range(),
        targets in prop::collection::vec(-1e6_f64..1e6, 1..20),
        seed in any::<u64>(),
    ) {
        let c = conv(&text, None);
        // A linear formula: inverted from its coefficients, never solved.
        prop_assert!(c.form().linear().is_some(), "{} not detected", text);
        let first: Vec<_> = targets.iter().map(|&t| c.to_raw_checked(t, raw)).collect();
        // A deterministic shuffle of the indices.
        let mut order: Vec<usize> = (0..targets.len()).collect();
        let mut s = seed | 1;
        for k in (1..order.len()).rev() {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            order.swap(k, usize::try_from(s % (k as u64 + 1)).unwrap_or(0));
        }
        for &k in &order {
            prop_assert_eq!(c.to_raw_checked(targets[k], raw), first[k]);
        }
    }
}

/// `a > b` for two raw values of one kind.
fn raw_gt(a: Raw, b: Raw) -> bool {
    match (a, b) {
        (Raw::Int(x), Raw::Int(y)) => x > y,
        (Raw::F32(x), Raw::F32(y)) => x > y,
        (Raw::F64(x), Raw::F64(y)) => x > y,
        _ => false,
    }
}
