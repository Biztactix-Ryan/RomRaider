//! Edit validation (US-RR-24-11): one test per [`Reject`] variant through
//! the public [`Conversion::to_raw_checked`], 4-byte extremes, division by
//! zero at and inside the range, the reasons and codes, and the property
//! that a value is either written inside the raw range or refused with a
//! reason that says where the range is. Every expected value is worked by
//! hand in the comment beside it.

use super::*;
use crate::conversion::{Conversion, ConversionSpec, Form, FormSpec, Linear, ToRealError};
use crate::diag::{Provenance, Severity, Span};
use crate::encoding::{RawRange, Storage};
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

fn lin(m: f64, b: f64) -> Conversion {
    Conversion::new(Form::Linear(Linear::new(m, b).unwrap()), None, None)
}

fn r(s: Storage) -> RawRange {
    s.raw_range()
}

fn here() -> Provenance {
    Provenance::new("defs.xml", Span::line_col(2, 5))
}

// ---------------------------------------------------------------------------
// OutOfRange
// ---------------------------------------------------------------------------

#[test]
fn out_of_range_on_four_byte_storage_is_refused_never_wrapped() {
    // x over u32: 0 ..= 4294967295. Within half a step of the top: the top.
    let c = conv("x");
    let u32r = r(Storage::U32);
    assert_eq!(
        c.to_raw_checked(4_294_967_295.4, u32r),
        Ok(Raw::Int(4_294_967_295))
    );
    // 2^32 is the raw value one past the top; a Java (int) cast of
    // Math.round(2^32) gave 0 (wrapped). Refused, with the range.
    let want = Reject::OutOfRange {
        target: 4_294_967_296.0,
        min: 0.0,
        max: 4_294_967_295.0,
        nearest: Raw::Int(4_294_967_295),
    };
    assert_eq!(c.to_raw_checked(4_294_967_296.0, u32r), Err(want));
    // Exactly half a step above the top: the half-up rule's 2^32, refused.
    assert!(matches!(
        c.to_raw_checked(4_294_967_295.5, u32r),
        Err(Reject::OutOfRange { .. })
    ));
    // Far past: still the same end, never a wrapped value.
    assert!(matches!(
        c.to_raw_checked(1e300, u32r),
        Err(Reject::OutOfRange {
            nearest: Raw::Int(4_294_967_295),
            ..
        })
    ));
    // Half a step below 0 rounds up to 0 (accepted); more is refused.
    assert_eq!(c.to_raw_checked(-0.5, u32r), Ok(Raw::Int(0)));
    assert_eq!(
        c.to_raw_checked(-0.6, u32r),
        Err(Reject::OutOfRange {
            target: -0.6,
            min: 0.0,
            max: 4_294_967_295.0,
            nearest: Raw::Int(0),
        })
    );
}

#[test]
fn out_of_range_on_a_decreasing_four_byte_scaling_names_both_ends_the_right_way_round() {
    // -x over i32: raw -2147483648 is the largest value, 2147483648; raw
    // 2147483647 the smallest, -2147483647 (MF-13).
    let c = conv("-x");
    let i32r = r(Storage::I32);
    let (min, max) = (-2_147_483_647.0, 2_147_483_648.0);
    assert_eq!(
        c.to_raw_checked(2_147_483_649.0, i32r),
        Err(Reject::OutOfRange {
            target: 2_147_483_649.0,
            min,
            max,
            nearest: Raw::Int(-2_147_483_648),
        })
    );
    assert_eq!(
        c.to_raw_checked(-2_147_483_648.0, i32r),
        Err(Reject::OutOfRange {
            target: -2_147_483_648.0,
            min,
            max,
            nearest: Raw::Int(2_147_483_647),
        })
    );
    assert_eq!(
        c.to_raw_checked(2_147_483_648.0, i32r),
        Ok(Raw::Int(-2_147_483_648))
    );
    let err = c.to_raw_checked(-3e9, i32r).unwrap_err();
    assert_eq!(
        err.to_string(),
        "-3000000000 is outside what the cell can hold, -2147483647 to 2147483648; the nearest \
         end is raw value 2147483647"
    );
}

#[test]
fn out_of_range_of_a_solved_four_byte_formula_reports_the_solved_ends() {
    // sqrt(x) over u32: 0 (raw 0) up to sqrt(4294967295) (raw 4294967295),
    // solved over a sampled scan.
    let c = conv("sqrt(x)");
    let u32r = r(Storage::U32);
    let top = 4_294_967_295_f64.sqrt();
    assert_eq!(
        c.to_raw_checked(70_000.0, u32r),
        Err(Reject::OutOfRange {
            target: 70_000.0,
            min: 0.0,
            max: top,
            nearest: Raw::Int(4_294_967_295),
        })
    );
}

#[test]
fn out_of_range_where_one_end_overflows_reports_the_last_finite_value() {
    // x + 1e308 over f64: finite from raw -f64::MAX (-0.797e308) up to the
    // last x with x + 1e308 <= f64::MAX; above that it overflows.
    let c = lin(1.0, 1e308);
    let f64r = r(Storage::F64);
    let Err(Reject::OutOfRange {
        target,
        min,
        max,
        nearest,
    }) = c.to_raw_checked(-f64::MAX, f64r)
    else {
        panic!("expected OutOfRange")
    };
    assert_eq!(target, -f64::MAX);
    assert_eq!(nearest, Raw::F64(-f64::MAX));
    assert_eq!(min, -f64::MAX + 1e308);
    assert!(max.is_finite() && max > 1.79e308, "{max}");
    // The last raw value with a value: the next float up overflows.
    let Raw::F64(below) = c.to_raw_checked(max, f64r).unwrap() else {
        panic!()
    };
    let next = f64::from_bits(below.to_bits() + 1);
    assert_eq!(c.to_real(below), Ok(max));
    assert!(c.to_real(next).is_err());
    // Mirrored: x - 1e308 overflows below, and is refused above the top.
    let d = lin(1.0, -1e308);
    let Err(Reject::OutOfRange { min, max, .. }) = d.to_raw_checked(f64::MAX, f64r) else {
        panic!("expected OutOfRange")
    };
    assert_eq!(max, f64::MAX - 1e308);
    assert!(min.is_finite() && min < -1.79e308, "{min}");
}

// ---------------------------------------------------------------------------
// NotInvertible
// ---------------------------------------------------------------------------

#[test]
fn not_invertible_conversions_are_refused_whatever_the_value() {
    // (x-128)^2 turns at 128.
    let turn = conv("(x-128)^2").to_raw_checked(4.0, r(Storage::U8));
    assert!(matches!(
        turn,
        Err(Reject::NotInvertible(NotInvertible::NotMonotonic { .. }))
    ));
    // 1/(x-10) over u8 divides by zero at raw 10, inside the range: a hole,
    // refused before any value (even one far from the pole) is looked at.
    let pole = conv("1/(x-10)");
    for target in [0.01, 1.0, 1e6, f64::NAN] {
        assert_eq!(
            pole.to_raw_checked(target, r(Storage::U8)),
            Err(Reject::NotInvertible(NotInvertible::Gap {
                at: Undefined::new(Raw::Int(10), ToRealError::NonFinite(f64::INFINITY))
            })),
            "{target}"
        );
    }
    // No value anywhere.
    let none = conv("sqrt(x-1000)").to_raw_checked(1.0, r(Storage::U8));
    assert!(matches!(
        none,
        Err(Reject::NotInvertible(NotInvertible::NoRange(_)))
    ));
}

// ---------------------------------------------------------------------------
// Singular
// ---------------------------------------------------------------------------

#[test]
fn singular_conversions_are_refused_even_at_their_one_value() {
    for (c, real) in [
        (lin(0.0, 7.0), 7.0),
        (conv("x*0+7"), 7.0),
        (conv("x-x+3"), 3.0),
    ] {
        assert_eq!(
            c.to_raw_checked(real, r(Storage::U16)),
            Err(Reject::Singular { real })
        );
    }
}

// ---------------------------------------------------------------------------
// NonFinite
// ---------------------------------------------------------------------------

#[test]
fn a_target_that_is_not_a_number_is_refused() {
    let c = conv("x*0.5");
    for t in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let got = c.to_raw_checked(t, r(Storage::U32));
        assert!(
            matches!(got, Err(Reject::NonFinite(NonFinite::Target(v))) if v.to_bits() == t.to_bits()),
            "{got:?}"
        );
    }
}

#[test]
fn a_target_only_reachable_near_a_pole_at_the_end_is_refused() {
    // 1/(x-255) over u8: the pole is the top raw value. Raw 0..=254 give
    // -1/255 down to -1 (raw 254); values below -1 lie only between raw
    // 254 and the pole, where no raw value is.
    let c = conv("1/(x-255)");
    let u8r = r(Storage::U8);
    assert_eq!(c.to_raw_checked(-1.0, u8r), Ok(Raw::Int(254)));
    // Within half the step back to raw 253 (-1/2): -1.2 is 0.2 past.
    assert_eq!(c.to_raw_checked(-1.2, u8r), Ok(Raw::Int(254)));
    let err = c.to_raw_checked(-1000.0, u8r).unwrap_err();
    assert_eq!(
        err,
        Reject::NonFinite(NonFinite::Beyond {
            target: -1000.0,
            limit: -1.0,
            raw: Raw::Int(254),
            beyond: Undefined::new(Raw::Int(255), ToRealError::NonFinite(f64::INFINITY)),
        })
    );
    assert_eq!(err.code(), codes::EDIT_NOT_FINITE);
    // 1/(x+128) over i8, the pole at the lowest raw value: mirrored.
    let d = conv("1/(x+128)");
    assert!(matches!(
        d.to_raw_checked(5.0, r(Storage::I8)),
        Err(Reject::NonFinite(NonFinite::Beyond {
            raw: Raw::Int(-127),
            beyond,
            ..
        })) if beyond.raw() == Raw::Int(-128)
    ));
}

#[test]
fn a_four_byte_division_by_zero_at_the_end_is_refused() {
    // 100000/x over u32: no value at raw 0; 100000 at raw 1.
    let c = conv("100000/x");
    let u32r = r(Storage::U32);
    assert_eq!(c.to_raw_checked(100_000.0, u32r), Ok(Raw::Int(1)));
    assert!(matches!(
        c.to_raw_checked(1e9, u32r),
        Err(Reject::NonFinite(NonFinite::Beyond {
            raw: Raw::Int(1),
            beyond,
            ..
        })) if beyond.raw() == Raw::Int(0)
    ));
    // Small values are near the top of the range; past it, out of range.
    assert!(matches!(
        c.to_raw_checked(1e-6, u32r),
        Err(Reject::OutOfRange {
            nearest: Raw::Int(4_294_967_295),
            ..
        })
    ));
}

// ---------------------------------------------------------------------------
// Codes, reasons, diagnostics
// ---------------------------------------------------------------------------

fn every_variant() -> [Reject; 8] {
    let undefined = Undefined::new(Raw::Int(0), ToRealError::NonFinite(f64::INFINITY));
    [
        Reject::OutOfRange {
            target: 300.0,
            min: 0.0,
            max: 255.0,
            nearest: Raw::Int(255),
        },
        NotInvertible::NotMonotonic {
            rise: Raw::Int(129),
            fall: Raw::Int(1),
        }
        .into(),
        NotInvertible::Gap { at: undefined }.into(),
        conv("sqrt(x-1000)")
            .to_raw_checked(1.0, r(Storage::U8))
            .unwrap_err(),
        Reject::Singular { real: 7.0 },
        NonFinite::Target(f64::NAN).into(),
        NonFinite::Beyond {
            target: 130.0,
            limit: 100.0,
            raw: Raw::Int(1),
            beyond: undefined,
        }
        .into(),
        NonFinite::NoValue { target: 2.0 }.into(),
    ]
}

#[test]
fn every_rejection_has_its_code_and_a_reason() {
    let want = [
        ("CNV0007", false),
        ("CNV0006", true),
        ("CNV0006", true),
        ("CNV0004", true),
        ("CNV0006", true),
        ("CNV0008", false),
        ("CNV0008", false),
        ("CNV0008", false),
    ];
    for (e, (code, wide)) in every_variant().into_iter().zip(want) {
        assert_eq!(e.code().as_str(), code, "{e:?}");
        assert_eq!(e.is_conversion_wide(), wide, "{e:?}");
        let d = e.to_diagnostic(here());
        assert_eq!(d.severity(), Severity::Error);
        assert_eq!(d.code(), e.code());
        assert_eq!(d.message(), e.to_string());
        assert!(!d.message().is_empty());
    }
}

#[test]
fn every_rejection_reads_as_a_reason() {
    let texts: Vec<String> = every_variant().iter().map(ToString::to_string).collect();
    assert_eq!(
        texts,
        [
            "300 is outside what the cell can hold, 0 to 255; the nearest end is raw value 255",
            "the conversion is not monotonic over the raw values the cell can hold (it rises at \
             raw value 129 and falls at raw value 1), so a real value can have more than one raw \
             value to choose from",
            "the conversion has no real value at raw value 0 (the conversion gives inf, which is \
             not a finite number), between raw values that have one, so it is not solved across \
             it",
            "no raw value the cell can hold has a real value (raw value 0: the conversion gives \
             NaN, which is not a finite number)",
            "every raw value converts to 7, so no raw value can be chosen for another",
            "NaN is not a finite number",
            "130 is beyond the last real value the conversion gives on that side, 100 (raw value \
             1): raw value 0 has none (the conversion gives inf, which is not a finite number)",
            "no raw value near 2 has a finite real value, so none can be chosen",
        ]
    );
    // Float raw values are written as floats.
    let f = Reject::OutOfRange {
        target: -1.0,
        min: 0.5,
        max: 2.5,
        nearest: Raw::F32(0.5),
    };
    assert_eq!(
        f.to_string(),
        "-1 is outside what the cell can hold, 0.5 to 2.5; the nearest end is raw value 0.5"
    );
    let g = Reject::OutOfRange {
        target: 3.0,
        min: 0.5,
        max: 2.5,
        nearest: Raw::F64(2.5),
    };
    assert_eq!(
        g.to_string(),
        "3 is outside what the cell can hold, 0.5 to 2.5; the nearest end is raw value 2.5"
    );
}

// ---------------------------------------------------------------------------
// Properties
// ---------------------------------------------------------------------------

fn storages() -> impl Strategy<Value = Storage> {
    prop::sample::select(vec![
        Storage::U8,
        Storage::I8,
        Storage::U16,
        Storage::I16,
        Storage::U32,
        Storage::I32,
        Storage::F32,
    ])
}

fn formulas() -> impl Strategy<Value = &'static str> {
    prop::sample::select(vec![
        "x",
        "-x",
        "x*0.25-50",
        "100-x/2",
        "x*1e9",
        "sqrt(x)",
        "x*x*x",
        "100/x",
        "1/(x-255)",
        "(x-128)^2",
        "x*0+4",
    ])
}

fn targets() -> impl Strategy<Value = f64> {
    prop_oneof![
        -1e12..1e12_f64,
        -300.0..300.0_f64,
        Just(f64::NAN),
        Just(f64::INFINITY),
        Just(f64::NEG_INFINITY),
        Just(4_294_967_296.0),
        Just(-2_147_483_649.0),
        Just(f64::MAX),
        Just(-f64::MAX),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    /// A value is either written as a raw value the cell holds, or refused
    /// with a reason consistent with it: never wrapped or clamped (EC-08).
    #[test]
    fn a_value_is_written_inside_the_range_or_refused_with_a_reason(
        formula in formulas(),
        storage in storages(),
        target in targets(),
    ) {
        let c = conv(formula);
        let raw = storage.raw_range();
        match c.to_raw_checked(target, raw) {
            Ok(got) => {
                prop_assert!(raw.contains(got), "{got:?} outside {raw:?}");
                prop_assert!(target.is_finite());
                prop_assert!(c.to_real_raw(got).is_ok());
            }
            Err(Reject::OutOfRange { target: t, min, max, nearest }) => {
                prop_assert_eq!(t.to_bits(), target.to_bits());
                prop_assert!(min <= max);
                prop_assert!(target < min || target > max, "{target} in {min}..={max}");
                prop_assert!(raw.contains(nearest));
                let end = c.to_real_raw(nearest).unwrap();
                prop_assert!(end == min || end == max);
                prop_assert!(raw.contains(nearest));
                // Refused at the end it is nearest.
                prop_assert_eq!(end, if target > max { max } else { min });
            }
            Err(Reject::NonFinite(NonFinite::Target(v))) => {
                prop_assert!(!target.is_finite());
                prop_assert_eq!(v.to_bits(), target.to_bits());
            }
            Err(e) => {
                // Conversion-wide refusals do not depend on the value.
                if e.is_conversion_wide() {
                    prop_assert_eq!(c.inverse(raw).err(), Some(e));
                }
            }
        }
    }
}
