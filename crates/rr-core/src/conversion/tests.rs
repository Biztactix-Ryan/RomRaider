//! Unit tests for conversions (US-RR-24-7): construction, refusals and
//! their diagnostics, and raw → real known answers. Every expected value is
//! hand-calculated; the working is in the comment beside it.

use super::*;
use crate::diag::{Provenance, Severity, Span, codes};
use crate::encoding::Raw;
use proptest::prelude::*;

fn spec(form: FormSpec<'_>) -> ConversionSpec<'_> {
    ConversionSpec {
        form,
        unit: None,
        format: None,
    }
}

fn expr_spec<'a>(to_real: &'a str, to_raw: Option<&'a str>) -> ConversionSpec<'a> {
    spec(FormSpec::Expr { to_real, to_raw })
}

fn build(s: &ConversionSpec<'_>) -> Conversion {
    Conversion::from_spec(s).unwrap()
}

fn refuse(s: &ConversionSpec<'_>) -> ConversionError {
    Conversion::from_spec(s).unwrap_err()
}

// ---------------------------------------------------------------------------
// Construction
// ---------------------------------------------------------------------------

#[test]
fn expr_spec_builds_an_expr_form_with_unit_and_format() {
    let c = build(&ConversionSpec {
        form: FormSpec::Expr {
            to_real: "x*0.25-50",
            to_raw: Some("(x+50)/0.25"),
        },
        unit: Some("degC"),
        format: Some("#.##"),
    });
    assert_eq!(c.unit(), Some("degC"));
    assert_eq!(c.format().map(DisplayFormat::pattern), Some("#.##"));
    assert_eq!(c.format().map(ToString::to_string).as_deref(), Some("#.##"));
    let Form::Expr(e) = c.form() else {
        panic!("expected an Expr form, got {:?}", c.form());
    };
    assert_eq!(e.to_real_expr().to_string(), "x*0.25-50.0");
    assert_eq!(
        e.to_raw().map(ToString::to_string).as_deref(),
        Some("(x+50.0)/0.25")
    );
}

#[test]
fn missing_unit_format_and_to_raw_stay_missing() {
    let c = build(&expr_spec("x", None));
    assert_eq!(c.unit(), None);
    assert_eq!(c.format(), None);
    let Form::Expr(e) = c.form() else {
        panic!("expected an Expr form");
    };
    assert!(e.to_raw().is_none());
}

#[test]
fn linear_spec_builds_a_linear_form() {
    let c = build(&spec(FormSpec::Linear { m: 0.25, b: -50.0 }));
    let Form::Linear(l) = *c.form() else {
        panic!("expected a Linear form");
    };
    assert_eq!((l.m(), l.b()), (0.25, -50.0));
    // A zero factor is a constant: allowed, it displays.
    assert!(Linear::new(0.0, 3.0).is_ok());
}

#[test]
fn lut_spec_builds_a_lut_form() {
    let points = [(0.0, 10.0), (10.0, 20.0)];
    let c = build(&spec(FormSpec::Lut(&points)));
    let Form::Lut(t) = c.form() else {
        panic!("expected a Lut form");
    };
    assert_eq!(t.points(), &points);
}

#[test]
fn new_matches_from_spec() {
    let form = Form::Linear(Linear::new(2.0, 1.0).unwrap());
    let c = Conversion::new(form, Some("V".to_owned()), Some(DisplayFormat::new("0.00")));
    assert_eq!(
        c,
        build(&ConversionSpec {
            form: FormSpec::Linear { m: 2.0, b: 1.0 },
            unit: Some("V"),
            format: Some("0.00"),
        })
    );
}

// ---------------------------------------------------------------------------
// Refusals and their diagnostics
// ---------------------------------------------------------------------------

#[test]
fn a_formula_that_does_not_parse_keeps_its_expr_code_and_span() {
    let e = refuse(&expr_spec("x*(2", None));
    assert!(matches!(
        e,
        ConversionError::Parse {
            direction: Direction::ToReal,
            ..
        }
    ));
    assert_eq!(e.code(), codes::EXPR_SYNTAX);
    let (direction, span) = e.span().unwrap();
    assert_eq!(direction, Direction::ToReal);
    assert_eq!(span.range(), 2..3); // the `(` that is never closed
    assert!(
        e.to_string()
            .starts_with("the to_real formula does not parse: ")
    );

    let e = refuse(&expr_spec("x", Some("sqrtt(x)")));
    assert_eq!(e.code(), codes::EXPR_BAD_CALL);
    assert_eq!(e.span().map(|(d, _)| d), Some(Direction::ToRaw));
    assert!(
        e.to_string()
            .starts_with("the to_raw formula does not parse: ")
    );
}

#[test]
fn a_parameter_reference_is_refused_pointing_at_the_first() {
    let e = refuse(&expr_spec("x+ P8 *[P9:km/h]", None));
    assert_eq!(
        e,
        ConversionError::ParamRef {
            direction: Direction::ToReal,
            param: "P8".to_owned(),
            span: ByteSpan::new(3, 5),
        }
    );
    assert_eq!(e.code(), codes::CONVERSION_PARAM_REF);
    assert_eq!(e.span(), Some((Direction::ToReal, ByteSpan::new(3, 5))));
    assert_eq!(
        e.to_string(),
        "the to_real formula refers to P8; a conversion formula may use only `x`, \
         the value being converted (names are case-sensitive)"
    );

    // An upper-case `X` is a reference, not the converted value.
    let e = refuse(&expr_spec("x", Some("X/2")));
    assert_eq!(
        e,
        ConversionError::ParamRef {
            direction: Direction::ToRaw,
            param: "X".to_owned(),
            span: ByteSpan::new(0, 1),
        }
    );

    let e = refuse(&expr_spec("[P9:km/h]", None));
    assert!(matches!(&e, ConversionError::ParamRef { param, .. } if param == "[P9:km/h]"));
}

#[test]
fn a_formula_without_x_is_a_constant_and_is_accepted() {
    let c = build(&expr_spec("5", Some("0")));
    assert_eq!(c.to_real(123.0), Ok(5.0));
}

#[test]
fn bad_lookup_tables_are_refused() {
    let cases: [(&[(f64, f64)], LutError); 7] = [
        (&[], LutError::TooFewPoints { found: 0 }),
        (&[(0.0, 1.0)], LutError::TooFewPoints { found: 1 }),
        (
            &[(0.0, 1.0), (f64::NAN, 2.0)],
            LutError::NonFinite { index: 1 },
        ),
        (
            &[(0.0, f64::INFINITY), (1.0, 2.0)],
            LutError::NonFinite { index: 0 },
        ),
        (
            &[(0.0, 1.0), (1.0, 2.0), (1.0, 3.0)],
            LutError::NotIncreasing { index: 2 },
        ),
        (
            &[(0.0, 1.0), (-1.0, 2.0)],
            LutError::NotIncreasing { index: 1 },
        ),
        // -0.0 is not greater than 0.0.
        (
            &[(0.0, 1.0), (-0.0, 2.0)],
            LutError::NotIncreasing { index: 1 },
        ),
    ];
    for (points, expected) in cases {
        let e = refuse(&spec(FormSpec::Lut(points)));
        assert_eq!(e, ConversionError::Lut(expected), "{points:?}");
        assert_eq!(e.code(), codes::INVALID_LUT);
        assert_eq!(e.span(), None);
    }
    assert_eq!(
        LutError::TooFewPoints { found: 1 }.to_string(),
        "the lookup table has 1 point(s); it needs at least 2"
    );
    assert_eq!(
        LutError::NonFinite { index: 0 }.to_string(),
        "point 1 of the lookup table is not a finite number"
    );
    assert_eq!(
        refuse(&spec(FormSpec::Lut(&[(0.0, 1.0), (0.0, 2.0)]))).to_string(),
        "the raw value of point 2 of the lookup table is not greater than that of point 1; \
         raw values must strictly increase"
    );
}

#[test]
fn non_finite_linear_coefficients_are_refused() {
    for (m, b) in [
        (f64::NAN, 0.0),
        (f64::INFINITY, 0.0),
        (1.0, f64::NEG_INFINITY),
        (1.0, f64::NAN),
    ] {
        let e = refuse(&spec(FormSpec::Linear { m, b }));
        assert!(matches!(e, ConversionError::Linear { .. }), "{m} {b}");
        assert_eq!(e.code(), codes::INVALID_LINEAR);
        assert_eq!(e.span(), None);
    }
    assert_eq!(
        Linear::new(1.0, f64::INFINITY).unwrap_err().to_string(),
        "a linear conversion needs a finite factor and offset, not m = 1, b = inf"
    );
}

#[test]
fn errors_become_error_diagnostics_with_their_code_and_message() {
    let provenance = Provenance::new("defs/a.xml", Span::line_col(3, 7));
    for e in [
        refuse(&expr_spec("(", None)),
        refuse(&expr_spec("P8", None)),
        refuse(&spec(FormSpec::Lut(&[]))),
        refuse(&spec(FormSpec::Linear {
            m: f64::NAN,
            b: 0.0,
        })),
    ] {
        let d = e.to_diagnostic(provenance.clone());
        assert_eq!(d.severity(), Severity::Error);
        assert_eq!(d.code(), e.code());
        assert_eq!(d.message(), e.to_string());
        assert_eq!(d.provenance(), &provenance);
        assert!(
            crate::diag::codes::lookup(d.code()).is_some(),
            "{}",
            d.code()
        );
    }
}

#[test]
fn direction_names() {
    assert_eq!(Direction::ToReal.to_string(), "to_real");
    assert_eq!(Direction::ToRaw.to_string(), "to_raw");
}

// ---------------------------------------------------------------------------
// Raw → real known answers
// ---------------------------------------------------------------------------

/// fixtures/kat/scaling-linear-u16-be.toml, as a formula and as a linear
/// form: 0.25 is exact in binary, so every value is exact.
#[test]
fn linear_u16_kat() {
    let formula = build(&expr_spec("x*0.25-50", Some("(x+50)/0.25")));
    let linear = build(&spec(FormSpec::Linear { m: 0.25, b: -50.0 }));
    for (raw, real) in [
        (0.0, -50.0),        // 0*0.25 - 50
        (200.0, 0.0),        // 200*0.25 = 50; 50 - 50
        (500.0, 75.0),       // 500*0.25 = 125; 125 - 50
        (65535.0, 16333.75), // 65535*0.25 = 16383.75; - 50
    ] {
        assert_eq!(formula.to_real(raw), Ok(real), "formula at {raw}");
        assert_eq!(linear.to_real(raw), Ok(real), "linear at {raw}");
    }
}

/// fixtures/kat/scaling-nonlinear-u8.toml: 14.7/(1+x/128).
#[test]
fn reciprocal_u8_kat() {
    let c = build(&expr_spec(
        "14.7/(1+x*0.0078125)",
        Some("(14.7/x-1)/0.0078125"),
    ));
    for (raw, real) in [
        (0.0, 14.7),                    // 14.7/1
        (64.0, 9.8),                    // 14.7/1.5
        (128.0, 7.35),                  // 14.7/2
        (255.0, 4.912_793_733_681_462), // 14.7*128/383
    ] {
        let got = c.to_real(raw).unwrap();
        assert!((got - real).abs() <= 1e-9, "at {raw}: {got} vs {real}");
    }
}

#[test]
fn linear_is_one_multiply_then_one_add() {
    // 3*7 + 2 = 23; a mutated operator gives 12, 1.428..., 19 or 42.
    let l = Linear::new(3.0, 2.0).unwrap();
    assert_eq!(l.to_real(7.0), Ok(23.0));
    assert_eq!(l.to_real(-1.0), Ok(-1.0)); // -3 + 2
}

#[test]
fn non_finite_results_are_errors_carrying_the_value() {
    // 1/(x-4) at x = 4 is 1/0 = +inf.
    let c = build(&expr_spec("1/(x-4)", None));
    assert_eq!(c.to_real(4.0), Err(ToRealError::NonFinite(f64::INFINITY)));
    assert_eq!(c.to_real(5.0), Ok(1.0));
    // 1e308*x overflows at x = 10.
    let l = Linear::new(1e308, 0.0).unwrap();
    assert_eq!(l.to_real(10.0), Err(ToRealError::NonFinite(f64::INFINITY)));
    assert_eq!(l.to_real(1.0), Ok(1e308));
    // NaN in, NaN out: an error, never 0.
    assert!(matches!(
        l.to_real(f64::NAN),
        Err(ToRealError::NonFinite(v)) if v.is_nan()
    ));
    assert_eq!(
        ToRealError::NonFinite(f64::INFINITY).to_string(),
        "the conversion gives inf, which is not a finite number"
    );
    assert_eq!(
        ToRealError::Eval(EvalError::Bindings {
            expected: 0,
            found: 1
        })
        .to_string(),
        EvalError::Bindings {
            expected: 0,
            found: 1
        }
        .to_string()
    );
}

/// A decreasing three-point table: (0, 100), (10, 50), (20, 40).
fn decreasing() -> Lut {
    Lut::new(&[(0.0, 100.0), (10.0, 50.0), (20.0, 40.0)]).unwrap()
}

#[test]
fn lut_is_exact_at_its_points() {
    let t = decreasing();
    assert_eq!(t.to_real(0.0), Ok(100.0));
    assert_eq!(t.to_real(10.0), Ok(50.0));
    assert_eq!(t.to_real(20.0), Ok(40.0));
    // Points whose interpolation from a neighbour would round differently:
    // A point is its own value, not the end of the line from the point
    // before: 1.1 + (0.3 - 1.1) * 1 is 0.30000000000000004, not 0.3.
    let t = Lut::new(&[(0.0, 1.1), (1.0, 0.3), (2.0, 0.1)]).unwrap();
    assert_eq!(t.to_real(1.0), Ok(0.3));
    assert_eq!(t.to_real(2.0), Ok(0.1));
}

#[test]
fn lut_interpolates_linearly_between_points() {
    let t = decreasing();
    // 100 + (50 - 100) * (2.5 / 10) = 100 - 12.5
    assert_eq!(t.to_real(2.5), Ok(87.5));
    // 50 + (40 - 50) * (5 / 10) = 45
    assert_eq!(t.to_real(15.0), Ok(45.0));
    // 50 + (40 - 50) * (9 / 10) = 50 - 9
    assert_eq!(t.to_real(19.0), Ok(41.0));
    // Uneven spacing: (0, 0), (4, 8): at 1, 0 + 8 * (1/4) = 2.
    let t = Lut::new(&[(-4.0, 0.0), (0.0, 0.0), (4.0, 8.0)]).unwrap();
    assert_eq!(t.to_real(1.0), Ok(2.0));
    assert_eq!(t.to_real(-2.0), Ok(0.0));
}

#[test]
fn lut_refuses_raw_values_outside_its_points() {
    let t = decreasing();
    for x in [-0.5, 20.5, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        match t.to_real(x) {
            Err(ToRealError::OutsideTable { raw, min, max }) => {
                assert!(raw.to_bits() == x.to_bits(), "{raw} vs {x}");
                assert_eq!((min, max), (0.0, 20.0));
            }
            other => panic!("{x}: expected OutsideTable, got {other:?}"),
        }
    }
    assert_eq!(
        t.to_real(21.0).unwrap_err().to_string(),
        "raw value 21 is outside the lookup table, which covers 0 to 20"
    );
}

#[test]
fn lut_interpolation_overflow_is_an_error() {
    // (1.7e308 - -1.7e308) overflows to inf before it is scaled.
    let t = Lut::new(&[(0.0, -1.7e308), (2.0, 1.7e308)]).unwrap();
    assert_eq!(t.to_real(1.0), Err(ToRealError::NonFinite(f64::INFINITY)));
}

#[test]
fn to_real_raw_converts_every_raw_kind() {
    let c = build(&spec(FormSpec::Linear { m: 0.5, b: 1.0 }));
    assert_eq!(c.to_real_raw(Raw::Int(-6)), Ok(-2.0)); // -3 + 1
    assert_eq!(c.to_real_raw(Raw::F32(1.5)), Ok(1.75)); // 0.75 + 1
    assert_eq!(c.to_real_raw(Raw::F64(4.0)), Ok(3.0)); // 2 + 1
    // 2^53 + 1 rounds to 2^53 (ties to even), as a Java long → double did.
    let id = build(&expr_spec("x", None));
    assert_eq!(
        id.to_real_raw(Raw::Int((1 << 53) + 1)),
        Ok(9_007_199_254_740_992.0)
    );
}

#[test]
fn every_form_converts_through_conversion_and_form() {
    let points = [(0.0, 0.0), (10.0, 5.0)];
    for (s, x, real) in [
        (spec(FormSpec::Linear { m: 2.0, b: 0.0 }), 3.0, 6.0),
        (expr_spec("x*x", None), 3.0, 9.0),
        (spec(FormSpec::Lut(&points)), 3.0, 1.5),
    ] {
        let c = build(&s);
        assert_eq!(c.to_real(x), Ok(real), "{s:?}");
        assert_eq!(c.form().to_real(x), Ok(real), "{s:?}");
    }
}

proptest! {
    /// Any finite raw value inside a table's range converts, and the result
    /// lies between the real values of the two points around it.
    #[test]
    fn lut_result_lies_between_its_neighbours(
        ys in proptest::collection::vec(-1.0e6f64..1.0e6, 2..8),
        frac in 0.0f64..=1.0,
    ) {
        let points: Vec<(f64, f64)> = ys
            .iter()
            .enumerate()
            .map(|(i, &y)| (i as f64 * 3.0, y))
            .collect();
        let t = Lut::new(&points).unwrap();
        let x = frac * points[points.len() - 1].0;
        let y = t.to_real(x).unwrap();
        let i = ((x / 3.0).floor() as usize).min(points.len() - 2);
        let (lo, hi) = (points[i].1.min(points[i + 1].1), points[i].1.max(points[i + 1].1));
        prop_assert!(lo <= y && y <= hi, "{x} -> {y}, not within {lo}..={hi}");
    }

    /// Converting is a pure function: the same raw value gives the same
    /// result however many times, and in whatever order, it is asked.
    #[test]
    fn to_real_is_order_independent(xs in proptest::collection::vec(-300.0f64..300.0, 1..20)) {
        let c = build(&expr_spec("if(x>0,14.7/(1+x*0.0078125),x*2)", None));
        let first: Vec<_> = xs.iter().map(|&x| c.to_real(x)).collect();
        let reversed: Vec<_> = xs.iter().rev().map(|&x| c.to_real(x)).collect();
        let again: Vec<_> = reversed.into_iter().rev().collect();
        prop_assert_eq!(first, again);
    }
}
