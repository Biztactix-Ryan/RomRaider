//! Linear-form detection (US-RR-24-8): which formulas are `m * x + b`,
//! with which coefficients. Every expected `m` and `b` is worked by hand
//! beside it, in the `f64` operations the detector performs.

use super::*;
use crate::expr;

fn detected(text: &str) -> Option<(f64, f64)> {
    detect(&expr::parse(text).unwrap()).map(|l| (l.m(), l.b()))
}

#[test]
fn recognises_linear_forms_with_their_coefficients() {
    let cases: &[(&str, f64, f64)] = &[
        // x: 1*x + 0.
        ("x", 1.0, 0.0),
        // Parentheses are not nodes.
        ("((x))", 1.0, 0.0),
        // x*0.25 - 50.
        ("x*0.25-50", 0.25, -50.0),
        // 0.25*x + 50: a constant on the left of `*` and `+`.
        ("0.25*x+50", 0.25, 50.0),
        // 50 + x*0.25.
        ("50+x*0.25", 0.25, 50.0),
        // x*(5/4) - 40: 5/4 folds to 1.25.
        ("x*(5/4)-40", 1.25, -40.0),
        // (x-40)/2: m = 1/2, b = -40/2 = -20.
        ("(x-40)/2", 0.5, -20.0),
        // 100 - x/2: m = -(1/2), b = 100 - 0.
        ("100-x/2", -0.5, 100.0),
        // x/256*100: m = (1/256)*100 = 0.390625 (exact: 100/256).
        ("x/256*100", 0.390_625, 0.0),
        // 14.7*(x/128): m = 14.7 * 0.0078125, b = 14.7 * 0.
        ("14.7*(x/128)", 14.7 * 0.007_812_5, 0.0),
        // -x: m = -1, b = -0.
        ("-x", -1.0, -0.0),
        // -(x+3)*2: m = -1*2, b = -3*2.
        ("-(x+3)*2", -2.0, -6.0),
        // x - x/4: m = 1 - 0.25, b = 0 - 0.
        ("x-x/4", 0.75, 0.0),
        // x/2 + x/4 + 1: m = 0.5 + 0.25, b = 0 + 0 + 1.
        ("x/2+x/4+1", 0.75, 1.0),
        // x*2^8: 2^8 folds to 256.
        ("x*2^8", 256.0, 0.0),
        // x*pow(2,-8): a function of constants folds (2^-8).
        ("x*pow(2,-8)", 0.003_906_25, 0.0),
        // x/(1+1): the divisor folds to 2.
        ("x/(1+1)", 0.5, 0.0),
        // x*(3>2): a comparison of constants folds to 1.
        ("x*(3>2)", 1.0, 0.0),
        // x*!0: `!` of a constant folds to 1.
        ("x*!0", 1.0, 0.0),
        // -(-x): m = 1, b = 0.
        ("-(-x)", 1.0, 0.0),
        // (x*0.1+10)-(x*0.05): m = 0.1 - 0.05, b = 10 - 0.
        ("(x*0.1+10)-(x*0.05)", 0.1 - 0.05, 10.0),
        // 3-(x-1): m = -1, b = 3 - (-1) = 4.
        ("3-(x-1)", -1.0, 4.0),
        // 1+(x+2): a constant plus a line with an offset: b = 1 + 2.
        ("1+(x+2)", 1.0, 3.0),
        // 2*(x+1): m = 1*2, b = 1*2.
        ("2*(x+1)", 2.0, 2.0),
        // (x+5)-(x+2)/2: m = 1 - 0.5, b = 5 - 2/2 = 4.
        ("(x+5)-(x+2)/2", 0.5, 4.0),
        // (x+6)/(x*0+2)... is not linear (a divisor in x); (x+6)/-2 is:
        // m = 1/-2, b = 6/-2.
        ("(x+6)/-2", -0.5, -3.0),
    ];
    for &(text, m, b) in cases {
        let got = detected(text);
        assert_eq!(got, Some((m, b)), "{text}");
        // And the sign of a zero offset is the operation's.
        let (_, gb) = got.unwrap();
        assert_eq!(
            gb.is_sign_negative(),
            b.is_sign_negative(),
            "{text}: sign of b"
        );
    }
}

#[test]
fn a_formula_without_x_is_a_constant() {
    assert_eq!(detected("7"), Some((0.0, 7.0)));
    assert_eq!(detected("2*3+1"), Some((0.0, 7.0)));
    assert_eq!(detected("sqrt(16)"), Some((0.0, 4.0)));
    // x*0 + 5 is linear with a zero factor: a constant.
    assert_eq!(detected("x*0+5"), Some((0.0, 5.0)));
    // x - x: the factors cancel.
    assert_eq!(detected("x-x"), Some((0.0, 0.0)));
}

#[test]
fn refuses_formulas_that_are_not_linear() {
    for text in [
        "x*x",
        "x/x",
        "1/x",
        "100/(x+1)",
        "x^2",
        "x^1",
        "2^x",
        "-x^2",
        "abs(x)",
        "sqrt(x)",
        "sum(x,1)",
        "x%3",
        "x>3",
        "x&255",
        "x|1",
        "x<<1",
        "!x",
        "if(x>1,x,0)",
        "BitWise(255,x,1)",
        "(x+1)*(x-1)",
        "x*(x+1)",
        "x/(x-x)",
        // Division by a zero constant: infinite or NaN everywhere.
        "x/0",
        "x/(1-1)",
        // Coefficients that are not finite.
        "x+1/0",
        "x*1e308*10",
        "x*(0/0)",
    ] {
        assert_eq!(detected(text), None, "{text}");
    }
}

#[test]
fn a_constant_divisor_of_x_is_not_mistaken_for_a_divisor_constant() {
    // 2/x is not linear, x/2 is: the operand order matters.
    assert_eq!(detected("2/x"), None);
    assert_eq!(detected("x/2"), Some((0.5, 0.0)));
    // 2-x and x-2 differ in both coefficients.
    assert_eq!(detected("2-x"), Some((-1.0, 2.0)));
    assert_eq!(detected("x-2"), Some((1.0, -2.0)));
    // x*3 and 3*x agree.
    assert_eq!(detected("3*x"), detected("x*3"));
    // x - (x*2): m = 1 - 2.
    assert_eq!(detected("x-(x*2)"), Some((-1.0, 0.0)));
    // 1+x and x+1 agree.
    assert_eq!(detected("1+x"), Some((1.0, 1.0)));
    // (x+1)+(x+2): m = 2, b = 3.
    assert_eq!(detected("(x+1)+(x+2)"), Some((2.0, 3.0)));
}

#[test]
fn a_function_of_x_among_constant_arguments_is_not_linear() {
    // The first argument is constant, the second is not.
    assert_eq!(detected("pow(2,x)"), None);
    assert_eq!(detected("atan2(1,x)"), None);
    // Constant arguments throughout fold.
    assert_eq!(detected("x*atan2(0,1)+abs(-3)"), Some((0.0, 3.0)));
}

#[test]
fn the_detected_form_agrees_with_the_formula() {
    // Where the formula's arithmetic is exact, m*x + b is the formula.
    for text in [
        "x*0.25-50",
        "(x-40)/2",
        "100-x/2",
        "x/256*100",
        "x*(5/4)-40",
    ] {
        let e = expr::parse(text).unwrap();
        let (m, b) = detected(text).unwrap();
        for x in [0.0, 1.0, 7.0, 128.0, 255.0, -3.0] {
            assert_eq!(e.eval(x, &[]).unwrap(), m * x + b, "{text} at {x}");
        }
    }
}
