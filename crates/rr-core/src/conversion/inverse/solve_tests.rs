//! Real → raw for non-linear conversions, by numeric solve (US-RR-24-9):
//! known answers worked by hand, decreasing and plateaued conversions,
//! the refusals (not monotonic, a gap, no value beyond the last value),
//! 4- and 8-byte and float storage, the check after a sampled scan, and
//! the round-trip and call-order properties over random monotonic
//! formulas and encodings.

use super::*;
use crate::conversion::{ConversionSpec, Coverage, FormSpec, Lut, Monotonicity, RangeError};
use crate::diag::{Provenance, Severity, Span};
use crate::encoding::{BitField, Storage};
use proptest::prelude::*;

fn conv(to_real: &str) -> Conversion {
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

fn storage(s: Storage) -> RawRange {
    s.raw_range()
}

fn u8r() -> RawRange {
    storage(Storage::U8)
}

fn int(c: &Conversion, real: f64, raw: RawRange) -> Result<i128, Reject> {
    c.to_raw_checked(real, raw).map(|r| match r {
        Raw::Int(v) => v,
        other => panic!("expected an integer, got {other:?}"),
    })
}

fn real(c: &Conversion, r: Raw) -> f64 {
    c.to_real_raw(r).unwrap()
}

// ---------------------------------------------------------------------------
// Known answers
// ---------------------------------------------------------------------------

#[test]
fn an_increasing_formula_is_solved_to_the_nearest_raw_value() {
    // x*x over u8: 0, 1, 4, 9, ..., 65025.
    let c = conv("x*x");
    assert_eq!(c.form().linear(), None);
    assert_eq!(int(&c, 4.0, u8r()), Ok(2));
    // 5 is 1 from 4 (raw 2), 4 from 9 (raw 3).
    assert_eq!(int(&c, 5.0, u8r()), Ok(2));
    assert_eq!(int(&c, 8.0, u8r()), Ok(3));
    // 6.5 is 2.5 from both: the larger raw value, as for linear forms.
    assert_eq!(int(&c, 6.5, u8r()), Ok(3));
    assert_eq!(int(&c, 0.0, u8r()), Ok(0));
    assert_eq!(int(&c, 65025.0, u8r()), Ok(255));
    // Every raw value comes back to itself.
    for r in 0..=255 {
        assert_eq!(int(&c, (r * r) as f64, u8r()), Ok(r), "raw {r}");
    }
}

#[test]
fn a_decreasing_formula_is_solved_the_right_way_round() {
    // 10000/(x+1): raw 0 is 10000, raw 1 is 5000, raw 99 is 100, raw 255
    // is 39.0625.
    let c = conv("10000/(x+1)");
    assert_eq!(int(&c, 10000.0, u8r()), Ok(0));
    assert_eq!(int(&c, 5000.0, u8r()), Ok(1));
    assert_eq!(int(&c, 100.0, u8r()), Ok(99));
    assert_eq!(int(&c, 39.0625, u8r()), Ok(255));
    // 7000 is 2000 from 5000 (raw 1) and 3000 from 10000 (raw 0).
    assert_eq!(int(&c, 7000.0, u8r()), Ok(1));
    // 7500 is halfway: the larger raw value.
    assert_eq!(int(&c, 7500.0, u8r()), Ok(1));
    // 3333.3: raw 2 (3333.33...) is nearer than raw 1 or raw 3 (2500).
    assert_eq!(int(&c, 3333.3, u8r()), Ok(2));
}

#[test]
fn the_ends_follow_the_half_step_rule() {
    let c = conv("x*x");
    // Raw 254 is 64516 and raw 255 is 65025: a step of 509. Within half
    // a step above raw 255 it is accepted; exactly half is refused (the
    // nearest grid value would be 256); half a step below raw 0 (0.5,
    // the step to raw 1 being 1) is accepted, as for linear forms.
    assert_eq!(int(&c, 65025.0 + 254.0, u8r()), Ok(255));
    assert_eq!(
        c.to_raw_checked(65025.0 + 254.5, u8r()),
        Err(Reject::OutOfRange {
            target: 65279.5,
            min: 0.0,
            max: 65025.0,
            nearest: Raw::Int(255)
        })
    );
    assert_eq!(int(&c, -0.5, u8r()), Ok(0));
    assert!(matches!(
        c.to_raw_checked(-0.6, u8r()),
        Err(Reject::OutOfRange {
            min: 0.0,
            max: 65025.0,
            nearest: Raw::Int(0),
            ..
        })
    ));
    assert!(c.to_raw_checked(1e9, u8r()).is_err());
    // Decreasing: past raw 0 (the largest value) on the high side.
    let d = conv("10000/(x+1)");
    assert!(matches!(
        d.to_raw_checked(20000.0, u8r()),
        Err(Reject::OutOfRange {
            min: 39.0625,
            max: 10000.0,
            nearest: Raw::Int(0),
            ..
        })
    ));
    assert!(matches!(
        d.to_raw_checked(1.0, u8r()),
        Err(Reject::OutOfRange {
            min: 39.0625,
            max: 10000.0,
            nearest: Raw::Int(255),
            ..
        })
    ));
}

#[test]
fn a_plateau_resolves_to_the_raw_value_next_to_the_crossing() {
    // x - x%4: 0,0,0,0, 4,4,4,4, 8,...; non-strictly increasing.
    let c = conv("x-x%4");
    // A plateau's own value: its lowest raw value.
    assert_eq!(int(&c, 4.0, u8r()), Ok(4));
    assert_eq!(int(&c, 0.0, u8r()), Ok(0));
    assert_eq!(int(&c, 252.0, u8r()), Ok(252));
    // Between two plateaus, nearer the lower: the lower's highest raw
    // value (next to where the value is crossed).
    assert_eq!(int(&c, 5.0, u8r()), Ok(7));
    // Nearer the upper: the upper's lowest.
    assert_eq!(int(&c, 7.0, u8r()), Ok(8));
    // Halfway: the larger raw value.
    assert_eq!(int(&c, 6.0, u8r()), Ok(8));
    // Every raw value comes back to one that displays the same.
    for r in 0..=255_i128 {
        let v = real(&c, Raw::Int(r));
        let got = c.to_raw_checked(v, u8r()).unwrap();
        assert_eq!(real(&c, got), v, "raw {r}");
        assert_eq!(got, Raw::Int(r - r % 4), "raw {r}");
    }
    // Decreasing plateaus: 100 - (x - x%4). The value 96 is raws 4..7;
    // the lowest of them.
    let d = conv("100-(x-x%4)");
    assert_eq!(int(&d, 96.0, u8r()), Ok(4));
    // 97 is nearer 96 (raws 4..7) than 100 (raws 0..3): the end next to
    // the crossing, 4.
    assert_eq!(int(&d, 97.0, u8r()), Ok(4));
    assert_eq!(int(&d, 99.0, u8r()), Ok(3));
}

#[test]
fn a_lookup_table_is_solved_too() {
    // (0, 100) to (100, 0) to... a decreasing table over u8 raw 0..=100.
    let t = Conversion::new(
        Form::Lut(Lut::new(&[(0.0, 100.0), (50.0, 90.0), (100.0, 0.0)]).unwrap()),
        None,
        None,
    );
    assert_eq!(int(&t, 100.0, u8r()), Ok(0));
    assert_eq!(int(&t, 90.0, u8r()), Ok(50));
    // 45 is raw 75, halfway down the second segment.
    assert_eq!(int(&t, 45.0, u8r()), Ok(75));
    assert_eq!(int(&t, 0.0, u8r()), Ok(100));
    // Raw 101 is outside the table: no value beyond raw 100.
    assert_eq!(
        t.to_raw_checked(-1.0, u8r()),
        Err(Reject::NonFinite(NonFinite::Beyond {
            target: -1.0,
            limit: 0.0,
            raw: Raw::Int(100),
            beyond: Undefined::new(
                Raw::Int(101),
                ToRealError::OutsideTable {
                    raw: 101.0,
                    min: 0.0,
                    max: 100.0
                }
            ),
        }))
    );
}

// ---------------------------------------------------------------------------
// Refusals
// ---------------------------------------------------------------------------

#[test]
fn a_non_monotonic_formula_is_refused_with_a_diagnostic() {
    // (x-128)^2 falls from 16384 at raw 0 (first at raw 1), then rises
    // (first at raw 129).
    let c = conv("(x-128)^2");
    let err = c.to_raw_checked(100.0, u8r()).unwrap_err();
    assert_eq!(
        err,
        Reject::NotInvertible(NotInvertible::NotMonotonic {
            rise: Raw::Int(129),
            fall: Raw::Int(1)
        })
    );
    // Refused before the value is looked at, and by the inverse itself.
    assert_eq!(c.to_raw_checked(f64::NAN, u8r()), Err(err));
    assert_eq!(c.inverse(u8r()).unwrap_err(), err);
    assert_eq!(err.code(), codes::NOT_INVERTIBLE);
    assert_eq!(
        err.to_string(),
        "the conversion is not monotonic over the raw values the cell can hold (it rises at raw \
         value 129 and falls at raw value 1), so a real value can have more than one raw value \
         to choose from"
    );
    let d = err.to_diagnostic(Provenance::new("defs.xml", Span::line_col(3, 7)));
    assert_eq!(d.severity(), Severity::Error);
    assert_eq!(d.code().as_str(), "CNV0006");
    assert_eq!(d.message(), err.to_string());
    // x*x is monotonic over u8 but not over i8 (it falls to 0, then rises).
    let sq = conv("x*x");
    assert_eq!(
        sq.to_raw_checked(4.0, storage(Storage::I8)),
        Err(Reject::NotInvertible(NotInvertible::NotMonotonic {
            rise: Raw::Int(1),
            fall: Raw::Int(-127)
        }))
    );
    // A given to_raw does not stand in for the solve.
    let given = Conversion::from_spec(&ConversionSpec {
        form: FormSpec::Expr {
            to_real: "(x-128)^2",
            to_raw: Some("sqrt(x)+128"),
        },
        unit: None,
        format: None,
    })
    .unwrap();
    assert!(matches!(
        given.to_raw_checked(100.0, u8r()),
        Err(Reject::NotInvertible(NotInvertible::NotMonotonic { .. }))
    ));
}

#[test]
fn a_division_by_zero_inside_the_range_is_refused() {
    // 100/x over i8: -0.78 down to -100 (raw -1), no value at raw 0, then
    // 100 (raw 1) down.
    let c = conv("100/x");
    let err = c.to_raw_checked(50.0, storage(Storage::I8)).unwrap_err();
    assert_eq!(
        err,
        Reject::NotInvertible(NotInvertible::Gap {
            at: Undefined::new(Raw::Int(0), ToRealError::NonFinite(f64::INFINITY))
        })
    );
    assert_eq!(err.code(), codes::NOT_INVERTIBLE);
    assert_eq!(
        err.to_string(),
        "the conversion has no real value at raw value 0 (the conversion gives inf, which is not \
         a finite number), between raw values that have one, so it is not solved across it"
    );
    // 1/(x-10) over u8 both has a gap and turns; the gap is reported.
    assert!(matches!(
        conv("1/(x-10)").to_raw_checked(1.0, u8r()),
        Err(Reject::NotInvertible(NotInvertible::Gap { at })) if at.raw() == Raw::Int(10)
    ));
}

#[test]
fn a_division_by_zero_at_an_end_bounds_the_range() {
    // 100/x over u16: no value at raw 0; 100 at raw 1, 50 at raw 2.
    let c = conv("100/x");
    let u16r = storage(Storage::U16);
    assert_eq!(int(&c, 100.0, u16r), Ok(1));
    assert_eq!(int(&c, 50.0, u16r), Ok(2));
    // 120 is past 100 towards raw 0, which has no value; more than half
    // the step (25) beyond raw 1: refused, with the reason.
    let err = c.to_raw_checked(130.0, u16r).unwrap_err();
    assert_eq!(
        err,
        Reject::NonFinite(NonFinite::Beyond {
            target: 130.0,
            limit: 100.0,
            raw: Raw::Int(1),
            beyond: Undefined::new(Raw::Int(0), ToRealError::NonFinite(f64::INFINITY)),
        })
    );
    assert_eq!(err.code(), codes::EDIT_NOT_FINITE);
    assert_eq!(
        err.to_string(),
        "130 is beyond the last real value the conversion gives on that side, 100 (raw value 1): \
         raw value 0 has none (the conversion gives inf, which is not a finite number)"
    );
    // Within half the step, 1; exactly half (125) goes to the larger raw
    // value, the step away from the hole.
    assert_eq!(int(&c, 120.0, u16r), Ok(1));
    assert_eq!(int(&c, 125.0, u16r), Ok(1));
    assert!(c.to_raw_checked(125.1, u16r).is_err());
    // sqrt(x-10) has no value below raw 10.
    let s = conv("sqrt(x-10)");
    assert_eq!(int(&s, 0.0, u8r()), Ok(10));
    assert_eq!(int(&s, 1.0, u8r()), Ok(11));
    assert_eq!(int(&s, -0.5, u8r()), Ok(10));
    assert!(matches!(
        s.to_raw_checked(-0.51, u8r()),
        Err(Reject::NonFinite(NonFinite::Beyond { raw: Raw::Int(10), beyond, .. }))
            if beyond.raw() == Raw::Int(9)
    ));
    // sqrt(200-x) has none above raw 200.
    let t = conv("sqrt(200-x)");
    assert_eq!(int(&t, 0.0, u8r()), Ok(200));
    assert_eq!(int(&t, 1.0, u8r()), Ok(199));
    assert!(matches!(
        t.to_raw_checked(-0.6, u8r()),
        Err(Reject::NonFinite(NonFinite::Beyond { raw: Raw::Int(200), beyond, .. }))
            if beyond.raw() == Raw::Int(201)
    ));
}

#[test]
fn a_constant_or_valueless_formula_is_refused() {
    let k = conv("x-x+3");
    assert_eq!(
        k.to_raw_checked(3.0, u8r()),
        Err(Reject::Singular { real: 3.0 })
    );
    let none = conv("sqrt(x-1000)");
    let err = none.to_raw_checked(1.0, u8r()).unwrap_err();
    assert!(matches!(
        err,
        Reject::NotInvertible(NotInvertible::NoRange(RangeError::NoValue { .. }))
    ));
    assert_eq!(err.code(), codes::CONVERSION_NO_VALUE);
    assert_eq!(
        err.to_string(),
        none.real_range(u8r()).unwrap_err().to_string()
    );
    assert_eq!(
        err.to_diagnostic(Provenance::new("defs.xml", Span::line_col(1, 1)))
            .code()
            .as_str(),
        "CNV0004"
    );
}

// ---------------------------------------------------------------------------
// Wide and float storage
// ---------------------------------------------------------------------------

#[test]
fn four_byte_storage_is_solved_without_overflow() {
    // 2^32 raw values: sampled, never scanned one by one.
    assert_eq!(Domain::new(storage(Storage::U32)).len(), 1 << 32);
    // MF-05: legacy's search cast 2^32 to a Java int. Over u32 the scan is
    // sampled; the answers are exact.
    let c = conv("sqrt(x)");
    let u32r = storage(Storage::U32);
    for r in [
        0_i128,
        1,
        2,
        1_000_000,
        2_147_483_647,
        2_147_483_648,
        3_000_000_000,
        4_294_967_294,
        4_294_967_295,
    ] {
        let v = real(&c, Raw::Int(r));
        assert_eq!(int(&c, v, u32r), Ok(r), "raw {r}");
    }
    assert_eq!(int(&c, 1000.0, u32r), Ok(1_000_000));
    assert!(c.to_raw_checked(65_537.0, u32r).is_err());
    // A cube over i32: about ±9.9e27 at the ends.
    let cube = conv("x*x*x");
    let i32r = storage(Storage::I32);
    for r in [
        -2_147_483_648_i128,
        -2_147_483_647,
        -1,
        0,
        1,
        2_147_483_646,
        2_147_483_647,
    ] {
        let v = real(&cube, Raw::Int(r));
        assert_eq!(int(&cube, v, i32r), Ok(r), "raw {r}");
    }
    assert!(matches!(
        cube.to_raw_checked(1e28, i32r),
        Err(Reject::OutOfRange {
            nearest: Raw::Int(2_147_483_647),
            ..
        })
    ));
    assert!(matches!(
        cube.to_raw_checked(-1e28, i32r),
        Err(Reject::OutOfRange {
            nearest: Raw::Int(-2_147_483_648),
            ..
        })
    ));
}

#[test]
fn eight_byte_storage_is_solved_without_overflow() {
    // Beyond 2^53 neighbouring raw values share a real value: the answer
    // displays the same.
    let cube = conv("x*x*x");
    for s in [Storage::U64, Storage::I64] {
        let raw = storage(s);
        let (lo, hi) = s.int_range().unwrap();
        for r in [lo, lo + 1, -1, 0, 1, 1 << 40, hi - 1, hi] {
            if !raw.contains(Raw::Int(r)) {
                continue;
            }
            let v = real(&cube, Raw::Int(r));
            let got = cube.to_raw_checked(v, raw).unwrap();
            assert_eq!(real(&cube, got), v, "{s:?} raw {r}");
        }
    }
    // Exact where f64 is: 2^20 cubed is 2^60.
    assert_eq!(
        int(&cube, 2.0_f64.powi(60), storage(Storage::U64)),
        Ok(1 << 20)
    );
}

#[test]
fn float_storage_is_solved_in_float_order() {
    let cube = conv("x*x*x");
    let f32r = storage(Storage::F32);
    assert_eq!(cube.to_raw_checked(8.0, f32r), Ok(Raw::F32(2.0)));
    assert_eq!(cube.to_raw_checked(-8.0, f32r), Ok(Raw::F32(-2.0)));
    let v = real(&cube, Raw::F32(0.1));
    assert_eq!(cube.to_raw_checked(v, f32r), Ok(Raw::F32(0.1)));
    let top = real(&cube, Raw::F32(f32::MAX));
    assert_eq!(cube.to_raw_checked(top, f32r), Ok(Raw::F32(f32::MAX)));
    // exp overflows above about 709.78: no value beyond the last float
    // with one.
    let e = conv("exp(x)");
    assert!(matches!(
        e.to_raw_checked(f64::MAX, f32r),
        Err(Reject::NonFinite(NonFinite::Beyond { beyond, .. }))
            if matches!(beyond.error(), ToRealError::NonFinite(v) if v == f64::INFINITY)
    ));
    assert!(matches!(
        e.to_raw_checked(1e308, f32r),
        Ok(Raw::F32(x)) if x > 709.0 && x < 710.0
    ));
    // exp is exactly 1 in f64 for every float within about 5.6e-17 of 0:
    // a plateau, of which the lowest raw value is chosen.
    let one = e.to_raw_checked(1.0, f32r).unwrap();
    assert_eq!(real(&e, one), 1.0);
    let Raw::F32(x) = one else { panic!("{one:?}") };
    assert!(x < 0.0 && x > -6e-17, "{x}");
    assert!(real(&e, Raw::F32(f32::from_bits(x.to_bits() + 1))) < 1.0);
    // exp is 0 for every float below about -745: a plateau, whose lowest
    // raw value is the most negative float.
    assert_eq!(e.to_raw_checked(0.0, f32r), Ok(Raw::F32(-f32::MAX)));
}

#[test]
fn a_bitfield_with_holes_is_solved_over_its_storable_values() {
    // Mask 0xF0: 0, 16, ..., 240; x*x is 0, 256, 1024, ...
    let raw = RawRange::Int(BitField::new(0xF0, Some(0), false).unwrap().raw_range());
    let c = conv("x*x");
    assert_eq!(int(&c, 256.0, raw), Ok(16));
    // 400 is nearer 256 (raw 16) than 1024 (raw 32).
    assert_eq!(int(&c, 400.0, raw), Ok(16));
    assert_eq!(int(&c, 700.0, raw), Ok(32));
    assert_eq!(int(&c, 57600.0, raw), Ok(240));
}

// ---------------------------------------------------------------------------
// After a sampled scan
// ---------------------------------------------------------------------------

#[test]
fn a_spike_the_sample_missed_is_caught_around_the_answer() {
    // 2^32 raw values: sampled, never scanned one by one.
    assert_eq!(Domain::new(storage(Storage::U32)).len(), 1 << 32);
    // x, except at 2684354560 (= 1.25 * 2^31: on no grid or landmark of
    // the u32 sample), which is far above. The scan sees an increasing
    // formula; the bisection for 2684354560.4 ends at the spike, and the
    // raw value after it falls.
    let c = conv("x+if(x==2684354560,4e9,0)");
    let u32r = storage(Storage::U32);
    let range = c.real_range(u32r).unwrap();
    assert_eq!(range.monotonicity(), Monotonicity::Increasing);
    assert_eq!(range.coverage(), Coverage::Sampled);
    assert_eq!(
        c.to_raw_checked(2_684_354_560.4, u32r),
        Err(Reject::NotInvertible(NotInvertible::NotMonotonic {
            rise: Raw::Int(1),
            fall: Raw::Int(2_684_354_561)
        }))
    );
    // Away from the spike, answers are exact.
    assert_eq!(int(&c, 1_000_000.0, u32r), Ok(1_000_000));
    // A decreasing formula with a dip the sample missed: the raw value at
    // the dip rises back.
    let d = conv("-x-if(x==2684354560,4e9,0)");
    assert_eq!(
        d.to_raw_checked(-2_684_354_560.4, u32r),
        Err(Reject::NotInvertible(NotInvertible::NotMonotonic {
            rise: Raw::Int(2_684_354_561),
            fall: Raw::Int(1)
        }))
    );
}

#[test]
fn a_hole_the_sample_missed_is_caught_around_the_answer() {
    // 2^32 raw values: sampled, never scanned one by one.
    assert_eq!(Domain::new(storage(Storage::U32)).len(), 1 << 32);
    // x, with no value (0/0) at 2684354560 only.
    let c = conv("x+0/(x-2684354560)");
    let u32r = storage(Storage::U32);
    assert_eq!(c.real_range(u32r).unwrap().undefined(), None);
    // 2684354560.6 is nearest raw 2684354561, whose neighbour has no
    // value.
    assert!(matches!(
        c.to_raw_checked(2_684_354_560.6, u32r),
        Err(Reject::NotInvertible(NotInvertible::Gap { at })) if at.raw() == Raw::Int(2_684_354_560)
            && matches!(at.error(), ToRealError::NonFinite(v) if v.is_nan())
    ));
    // The hole's own value: none there, and 2684354561 is a whole step
    // away.
    assert!(matches!(
        c.to_raw_checked(2_684_354_560.0, u32r),
        Err(Reject::NonFinite(NonFinite::Beyond { beyond, .. })) if beyond.raw() == Raw::Int(2_684_354_560)
    ));
    assert_eq!(int(&c, 5.0, u32r), Ok(5));
}

#[test]
fn the_ends_of_a_sampled_run_are_found_by_bisection() {
    // 2^32 raw values: sampled, never scanned one by one.
    assert_eq!(Domain::new(storage(Storage::U32)).len(), 1 << 32);
    // sqrt(x-10) over u32: the sample sees no value at 8 and one at 12;
    // the run starts at 10.
    let s = conv("sqrt(x-10)");
    let u32r = storage(Storage::U32);
    assert_eq!(int(&s, 0.0, u32r), Ok(10));
    assert_eq!(int(&s, 1.0, u32r), Ok(11));
    // sqrt(3e9-x) has no value above 3e9; the run ends there.
    let t = conv("sqrt(3000000000-x)");
    assert_eq!(int(&t, 0.0, u32r), Ok(3_000_000_000));
    assert_eq!(int(&t, 1.0, u32r), Ok(2_999_999_999));
    assert!(matches!(
        t.to_raw_checked(-1.0, u32r),
        Err(Reject::NonFinite(NonFinite::Beyond {
            raw: Raw::Int(3_000_000_000),
            ..
        }))
    ));
}

#[test]
fn verify_checks_every_neighbour_in_the_run() {
    let d = Domain::new(u8r());
    let up = |x: f64| Ok(x);
    assert_eq!(verify(d, (0, 255), &up, 100, true, Raw::Int(1)), Ok(()));
    // At the ends of the run only the raw values inside it count.
    let bad = |x: f64| {
        if (10.0..=20.0).contains(&x) {
            Ok(x)
        } else {
            Ok(-x)
        }
    };
    assert_eq!(verify(d, (10, 20), &bad, 10, true, Raw::Int(1)), Ok(()));
    assert_eq!(verify(d, (10, 20), &bad, 20, true, Raw::Int(1)), Ok(()));
    // A fall into raw 102 is seen from up to two above it, or two below
    // the raw value before it (the step is between 101 and 102).
    let dip = |x: f64| Ok(if x == 102.0 { 0.0 } else { x });
    for i in [100, 101, 102, 103] {
        assert_eq!(
            verify(d, (0, 255), &dip, i, true, Raw::Int(1)),
            Err(Reject::NotInvertible(NotInvertible::NotMonotonic {
                rise: Raw::Int(1),
                fall: Raw::Int(102)
            })),
            "{i}"
        );
    }
    assert_eq!(verify(d, (0, 255), &dip, 99, true, Raw::Int(1)), Ok(()));
    assert_eq!(verify(d, (0, 255), &dip, 104, true, Raw::Int(1)), Ok(()));
    // A raw value with no value in the window is a gap.
    let hole = |x: f64| {
        if x == 52.0 {
            Err(ToRealError::NonFinite(f64::INFINITY))
        } else {
            Ok(x)
        }
    };
    assert_eq!(
        verify(d, (0, 255), &hole, 50, true, Raw::Int(1)),
        Err(Reject::NotInvertible(NotInvertible::Gap {
            at: Undefined::new(Raw::Int(52), ToRealError::NonFinite(f64::INFINITY))
        }))
    );
    assert_eq!(verify(d, (0, 255), &hole, 49, true, Raw::Int(1)), Ok(()));
    // Equal neighbours are no turn.
    let flat = |_: f64| Ok(1.0);
    assert_eq!(verify(d, (0, 255), &flat, 50, true, Raw::Int(1)), Ok(()));
    assert_eq!(verify(d, (0, 255), &flat, 50, false, Raw::Int(1)), Ok(()));
    // Decreasing: a rise is the turn.
    let down = |x: f64| Ok(-x);
    assert_eq!(verify(d, (0, 255), &down, 50, false, Raw::Int(1)), Ok(()));
    assert_eq!(
        verify(d, (0, 255), &up, 50, false, Raw::Int(7)),
        Err(Reject::NotInvertible(NotInvertible::NotMonotonic {
            rise: Raw::Int(49),
            fall: Raw::Int(7)
        }))
    );
    assert_eq!(
        verify(d, (0, 255), &down, 50, true, Raw::Int(7)),
        Err(Reject::NotInvertible(NotInvertible::NotMonotonic {
            rise: Raw::Int(7),
            fall: Raw::Int(49)
        }))
    );
}

#[test]
fn the_inverse_answers_as_to_raw_does() {
    let c = conv("x*x");
    let inv = c.inverse(u8r()).unwrap();
    for t in [
        0.0,
        3.0,
        4.0,
        6.5,
        100.0,
        65025.0,
        70000.0,
        -3.0,
        f64::INFINITY,
    ] {
        assert_eq!(inv.to_raw_checked(t), c.to_raw_checked(t, u8r()), "{t}");
    }
    assert!(
        matches!(inv.to_raw_checked(f64::NAN), Err(Reject::NonFinite(NonFinite::Target(v))) if v.is_nan())
    );
    assert_eq!(c.form().inverse(u8r()), c.inverse(u8r()));
}

// ---------------------------------------------------------------------------
// Properties
// ---------------------------------------------------------------------------

/// A non-linear formula of a shape definitions use, from its constants.
fn formula() -> impl Strategy<Value = String> {
    let k = prop_oneof![
        (1_u32..=1000).prop_map(|n| f64::from(n) / 100.0),
        (1_u32..=100).prop_map(f64::from),
    ];
    let c = (1_u32..=1000).prop_map(f64::from);
    (0_u8..7, k, c).prop_map(|(shape, k, c)| match shape {
        0 => format!("x*x*{k:?}+{c:?}"),
        1 => format!("sqrt(x)*{k:?}"),
        2 => format!("{c:?}*1000/(x+{c:?})"),
        3 => format!("exp(x/{c:?})*{k:?}"),
        4 => format!("x*x*x/{c:?}"),
        5 => format!("{c:?}-x*x/{k:?}"),
        _ => format!("x-x%{c:?}"),
    })
}

/// Raw ranges: exhaustive and sampled storages, and bitfields.
fn raw_range() -> impl Strategy<Value = RawRange> {
    prop_oneof![
        prop::sample::select(vec![
            Storage::U8,
            Storage::I8,
            Storage::U16,
            Storage::I16,
            Storage::U24,
            Storage::U32,
            Storage::I32,
        ])
        .prop_map(Storage::raw_range),
        (1_u64..=0xFFF, prop::option::of(0_u32..4), any::<bool>()).prop_filter_map(
            "a valid bitfield",
            |(mask, shift, signed)| {
                BitField::new(mask, shift, signed)
                    .ok()
                    .map(|f| RawRange::Int(f.raw_range()))
            }
        ),
    ]
}

/// `cases` cases, and little shrinking: each case scans a raw range of up
/// to `2^32` values, and a failure is reported with its formula and raw
/// value whether or not it is minimal.
fn config(cases: u32) -> ProptestConfig {
    ProptestConfig {
        cases,
        max_shrink_iters: 16,
        ..ProptestConfig::default()
    }
}

proptest! {
    #![proptest_config(config(96))]

    /// For every invertible formula and raw value r: to_raw(to_real(r))
    /// displays as r does, and is r itself unless a neighbour shares its
    /// real value (a plateau, or f64 cannot tell them apart).
    #[test]
    fn solved_formulas_round_trip(
        text in formula(),
        raw in raw_range(),
        picks in prop::collection::vec(any::<u64>(), 8),
    ) {
        let c = conv(&text);
        let Ok(inv) = c.inverse(raw) else { return Ok(()) };
        let d = Domain::new(raw);
        let (lo, hi) = inv.run;
        for p in picks {
            let span = u128::try_from(hi - lo).unwrap_or(0) + 1;
            let i = lo + i128::try_from(u128::from(p) % span).unwrap_or(0);
            let r = d.raw(i);
            let v = c.to_real_raw(r).unwrap();
            let got = inv.to_raw_checked(v);
            let shared = [i - 1, i + 1]
                .into_iter()
                .filter(|j| (lo..=hi).contains(j))
                .any(|j| c.to_real_raw(d.raw(j)) == Ok(v));
            if shared {
                prop_assert_eq!(got.map(|g| c.to_real_raw(g)), Ok(Ok(v)), "{} at {:?}", text, r);
            } else {
                prop_assert_eq!(got, Ok(r), "{} at {:?}", text, r);
            }
        }
    }
}

proptest! {
    // Each to_raw call scans the raw range afresh (up to 2^16 values): few
    // cases, each with a shuffled sequence.
    #![proptest_config(config(32))]

    /// The same question gets the same answer whatever was asked before and
    /// in whatever order, from to_raw or a reused inverse: no cache.
    #[test]
    fn solved_answers_do_not_depend_on_call_order(
        text in formula(),
        raw in raw_range(),
        targets in prop::collection::vec(-1e5_f64..1e7, 1..6),
        seed in any::<u64>(),
    ) {
        let c = conv(&text);
        let first: Vec<_> = targets.iter().map(|&t| c.to_raw_checked(t, raw)).collect();
        let mut order: Vec<usize> = (0..targets.len()).collect();
        let mut s = seed | 1;
        for k in (1..order.len()).rev() {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            order.swap(k, usize::try_from(s % (k as u64 + 1)).unwrap_or(0));
        }
        let inv = c.inverse(raw);
        // Compared as Debug text: a refusal can carry a NaN (`sqrt(x)` at
        // raw -1 is `NonFinite(NaN)`), which `==` never equals itself.
        let same = |a: &Result<Raw, Reject>, b: &Result<Raw, Reject>| {
            format!("{a:?}") == format!("{b:?}")
        };
        for &k in &order {
            let again = c.to_raw_checked(targets[k], raw);
            prop_assert!(same(&again, &first[k]), "{:?} != {:?}", again, first[k]);
            if let Ok(inv) = &inv {
                let reused = inv.to_raw_checked(targets[k]);
                prop_assert!(same(&reused, &first[k]), "{:?} != {:?}", reused, first[k]);
            }
        }
    }
}
