//! Known-answer and property tests for the evaluator (US-RR-20-7).
//!
//! Every expected value is worked out by hand from the rule it tests, and
//! each group cites that rule:
//!
//! * **jep.jar**: the bytecode of the named class and method in
//!   `legacy/lib/common/jep.jar` (`org/nfunk/jep/function/` unless said
//!   otherwise), as `Class.method@offsets` (read, never run, D-024);
//! * **legacy**: RomRaider's Java under `legacy/src/main/java/com/romraider`;
//! * **JLS** / **Math**: the Java Language Specification and the
//!   `java.lang.Math` javadoc, for the `double` and `int` semantics those
//!   classes use;
//! * **IEEE**: IEEE 754 double arithmetic, which Java and Rust share.
//!
//! Exact operations are compared bit for bit. Transcendental functions are
//! compared to within two ulps: `java.lang.Math` itself only promises one
//! ulp, so legacy was not bit-identical across JVMs either.

use core::f64::consts::{E, FRAC_PI_2, FRAC_PI_4, LN_10, PI};

use proptest::prelude::*;

use super::super::limits::tests::ROOMY;
use super::super::tests::{CORPUS_SAMPLE, Tree, any_tree};
use super::*;
use crate::expr::{Expr, NodeId, ParamRef, parse};

// ---- helpers ----

fn ok(src: &str) -> Expr {
    match parse(src) {
        Ok(e) => e,
        Err(err) => panic!("{src:?} should parse, got {err}"),
    }
}

/// The raw value of `src` at `x` with no parameters: the result, or the
/// value a [`EvalError::NonFinite`] carries.
fn value(src: &str, x: f64) -> f64 {
    raw(ok(src).eval(x, &[]))
}

fn raw(r: Result<f64, EvalError>) -> f64 {
    match r {
        Ok(v) | Err(EvalError::NonFinite(v)) => v,
        Err(e) => panic!("unexpected {e:?}"),
    }
}

/// Same value: identical bits, or both NaN.
fn same(a: f64, b: f64) -> bool {
    a.to_bits() == b.to_bits() || (a.is_nan() && b.is_nan())
}

/// `src` at `x` is exactly `expected` (NaN matches NaN; `-0` does not
/// match `0`).
#[track_caller]
fn kat(src: &str, x: f64, expected: f64) {
    let got = value(src, x);
    assert!(
        same(got, expected),
        "{src:?} at x={x:?}: got {got:?}, expected {expected:?}"
    );
}

/// `src` at `x` is within two ulps of `expected`.
#[track_caller]
fn kat_near(src: &str, x: f64, expected: f64) {
    let got = value(src, x);
    let ulps = got.to_bits().abs_diff(expected.to_bits());
    assert!(
        got.is_sign_positive() == expected.is_sign_positive() && ulps <= 2,
        "{src:?} at x={x:?}: got {got:?}, expected {expected:?}"
    );
}

const NAN: f64 = f64::NAN;
const INF: f64 = f64::INFINITY;

// ---- arithmetic (IEEE; jep.jar Add/Subtract/Multiply/Divide/UMinus) ----

#[test]
fn arithmetic_is_ieee_double() {
    kat("1+2", 0.0, 3.0);
    kat("0.1+0.2", 0.0, 0.300_000_000_000_000_04);
    kat("5-7", 0.0, -2.0);
    kat("6*7", 0.0, 42.0);
    kat("7/2", 0.0, 3.5);
    kat("-x", 5.0, -5.0);
    kat("-x", 0.0, -0.0);
    kat("--x", 3.0, 3.0);
    kat("+x", 3.0, 3.0);
    kat("1-2-3", 0.0, -4.0);
    kat("2*3+4", 0.0, 10.0);
    kat("2+3*4", 0.0, 14.0);
    kat("-2*3", 0.0, -6.0);
}

#[test]
fn division_by_zero_and_nan_follow_ieee() {
    kat("1/0", 0.0, INF);
    kat("-1/0", 0.0, -INF);
    kat("1/-0", 0.0, -INF);
    kat("0/0", 0.0, NAN);
    kat("x/x", 0.0, NAN);
    kat("x+1", NAN, NAN);
    kat("x*0", INF, NAN);
    kat("x-x", INF, NAN);
    // An infinite intermediate is fine; only the result is checked.
    kat("1/(1/0)", 0.0, 0.0);
    kat("1/(-1/0)", 0.0, -0.0);
}

// ---- % and mod (jep.jar Modulus.run@47-52: `drem`; JLS §15.17.3) ----

#[test]
fn remainder_takes_the_sign_of_the_dividend() {
    kat("7%3", 0.0, 1.0);
    kat("-7%3", 0.0, -1.0);
    kat("7%-3", 0.0, 1.0);
    kat("-7%-3", 0.0, -1.0);
    kat("5.5%2", 0.0, 1.5);
    kat("x%256", 65535.0, 255.0);
    kat("x%0", 5.0, NAN);
    kat("x%2", INF, NAN);
    kat("5%x", INF, 5.0);
    kat("-0%5", 0.0, -0.0);
    kat("mod(-7,3)", 0.0, -1.0);
    kat("mod(7.5,2)", 0.0, 1.5);
    kat("mod(1,0)", 0.0, NAN);
}

// ---- ^ and pow (jep.jar Power.power(Number,Number)@0-62; Math.pow) ----

#[test]
fn power_is_math_pow() {
    kat("2^10", 0.0, 1024.0);
    kat("2^-1", 0.0, 0.5);
    kat("2^0.5", 0.0, core::f64::consts::SQRT_2);
    kat("-2^2", 0.0, -4.0);
    kat("2^3^2", 0.0, 512.0);
    kat("(-2)^3", 0.0, -8.0);
    kat("(-2)^2", 0.0, 4.0);
    kat("(-2)^-1", 0.0, -0.5);
    kat("0^0", 0.0, 1.0);
    kat("0^0.5", 0.0, 0.0);
    kat("1^3", 0.0, 1.0);
    kat("x^0", NAN, 1.0);
    kat("x^2", NAN, NAN);
    kat("2^x", INF, INF);
    kat("0.5^x", INF, 0.0);
    kat("2^x", -INF, 0.0);
    kat("0^-1", 0.0, INF);
    kat("10^400", 0.0, INF);
    kat("pow(2,3)", 0.0, 8.0);
    kat("pow(9,0.5)", 0.0, 3.0);
}

#[test]
fn negative_base_to_a_non_int_power_is_nan() {
    // JEP goes complex (Power.power(Number,Number)@0-43); JEP.getValue@13-42
    // makes a complex result with a non-zero imaginary part NaN.
    kat("(-8)^(1/3)", 0.0, NAN);
    kat("(-2)^0.5", 0.0, NAN);
    kat("pow(-2,0.5)", 0.0, NAN);
    // 1e10 is a whole number but not an `int` (it saturates to
    // 2147483647), so JEP takes the complex branch.
    kat("(-2)^1e10", 0.0, NAN);
    kat("(-1)^x", INF, NAN);
    kat("(-2)^x", NAN, NAN);
    // -0 is not below zero.
    kat("(-0)^0.5", 0.0, 0.0);
    // A negative exponent on a positive base is ordinary.
    kat("4^-0.5", 0.0, 0.5);
}

#[test]
fn power_has_java_special_cases_where_c_differs() {
    // Math.pow: "If the second argument is NaN, then the result is NaN";
    // C's pow(1, NaN) is 1.
    kat("1^x", NAN, NAN);
    kat("2^x", NAN, NAN);
    // Math.pow: |first| = 1 and second infinite is NaN; C gives 1.
    kat("1^x", INF, NAN);
    kat("1^x", -INF, NAN);
    kat("1^(1/0)", 0.0, NAN);
    // |base| = 1 with a finite exponent is ordinary.
    kat("1^0.5", 0.0, 1.0);
    kat("(-1)^3", 0.0, -1.0);
    kat("(-1)^2", 0.0, 1.0);
}

// ---- comparisons (jep.jar Comparative.run@123-157, lt/gt/le/ge/eq/ne) ----

#[test]
fn comparisons_give_one_or_zero() {
    for (src, lt, eq, gt) in [
        ("<", 1.0, 0.0, 0.0),
        (">", 0.0, 0.0, 1.0),
        ("<=", 1.0, 1.0, 0.0),
        (">=", 0.0, 1.0, 1.0),
        ("==", 0.0, 1.0, 0.0),
        ("!=", 1.0, 0.0, 1.0),
    ] {
        kat(&format!("1{src}2"), 0.0, lt);
        kat(&format!("2{src}2"), 0.0, eq);
        kat(&format!("3{src}2"), 0.0, gt);
    }
    // Exact, with no tolerance (Comparative.eq@104-147 compares the two
    // doubles with `dcmpl`; the 1e-6 tolerance is for complex operands).
    kat("0.1+0.2==0.3", 0.0, 0.0);
    kat("1.0000001==1", 0.0, 0.0);
    kat("-0==0", 0.0, 1.0);
}

#[test]
fn comparisons_with_nan_are_false_except_not_equal() {
    kat("x<1", NAN, 0.0);
    kat("x>1", NAN, 0.0);
    kat("x<=1", NAN, 0.0);
    kat("x>=1", NAN, 0.0);
    kat("x==x", NAN, 0.0);
    kat("x!=x", NAN, 1.0);
    kat("1<x", NAN, 0.0);
    kat("1>=x", NAN, 0.0);
}

// ---- logic (jep.jar Logical.run@76-121, Not.run@17-34) ----

#[test]
fn logic_treats_non_zero_as_true() {
    kat("1&&0", 0.0, 0.0);
    kat("0&&1", 0.0, 0.0);
    kat("0&&0", 0.0, 0.0);
    kat("2&&-1", 0.0, 1.0);
    kat("0||0", 0.0, 0.0);
    kat("0||3", 0.0, 1.0);
    kat("3||0", 0.0, 1.0);
    kat("-1||-1", 0.0, 1.0);
    kat("!0", 0.0, 1.0);
    kat("!-0", 0.0, 1.0);
    kat("!5", 0.0, 0.0);
    kat("!!7", 0.0, 1.0);
    // NaN is not 0, so it is true.
    kat("x&&1", NAN, 1.0);
    kat("1&&x", NAN, 1.0);
    kat("x||0", NAN, 1.0);
    kat("0||x", NAN, 1.0);
    kat("!x", NAN, 0.0);
}

// ---- if (jep.jar If.evaluate@139-199) ----

#[test]
fn if_with_three_arguments() {
    kat("if(x,10,20)", 1.0, 10.0);
    kat("if(x,10,20)", 0.5, 10.0);
    kat("if(x,10,20)", 0.0, 20.0);
    kat("if(x,10,20)", -0.0, 20.0);
    kat("if(x,10,20)", -1.0, 20.0);
    kat("if(x,10,20)", NAN, 20.0);
    kat("if(x>32767,x-65536,x)", 65535.0, -1.0);
    kat("if(x>32767,x-65536,x)", 32767.0, 32767.0);
}

#[test]
fn if_with_four_arguments() {
    kat("if(x,10,20,30)", 1.0, 10.0);
    kat("if(x,10,20,30)", -1.0, 20.0);
    kat("if(x,10,20,30)", 0.0, 30.0);
    kat("if(x,10,20,30)", -0.0, 30.0);
    kat("if(x,10,20,30)", NAN, 30.0);
}

#[test]
fn if_takes_the_branch_value_even_when_the_other_is_not_finite() {
    // JEP evaluates only the branch taken; evaluating both gives the same
    // value because nothing can fail part-way.
    kat("if(x,1/0,2)", 0.0, 2.0);
    kat("if(x,1,0/0)", 1.0, 1.0);
    kat("if(x,1/0,2)", 1.0, INF);
}

// ---- sum (jep.jar Sum.run@23-58) ----

#[test]
fn sum_adds_from_the_last_argument_back() {
    kat("sum(5)", 0.0, 5.0);
    kat("sum(1,2)", 0.0, 3.0);
    kat("sum(1,2,3)", 0.0, 6.0);
    kat("sum(1,-2,3,-4)", 0.0, -2.0);
    // 1 + (1e16 + -1e16) = 1; adding from the front would give
    // (1 + 1e16) + -1e16 = 0, as 1e16 + 1 rounds to 1e16.
    kat("sum(1,1e16,-1e16)", 0.0, 1.0);
    kat("sum(1e16,-1e16,1)", 0.0, 0.0);
    kat("sum(x,1)", NAN, NAN);
}

// ---- log, ln, exp, sqrt (jep.jar Logarithm, NaturalLogarithm, Exp,
// SquareRoot) ----

#[test]
fn log_is_natural_log_over_ln_10() {
    // Logarithm.log@36-57: Math.log(a) / Math.log(10.0).
    kat("log(10)", 0.0, 1.0);
    kat("log(100)", 0.0, 2.0);
    // ln(1000) / ln(10), not log10's 3.
    kat("log(1000)", 0.0, 2.999_999_999_999_999_6);
    kat("log(1)", 0.0, 0.0);
    kat("log(x)", INF, INF);
    // The complex branch (Logarithm.log@75-96): log(0) divides (-inf, 0) by
    // (ln 10, 0), and Complex.div@18-76 makes the imaginary part
    // (0 - 0 * -inf) / ln 10 = NaN, so JEP.getValue@25-37 gives NaN.
    kat("log(0)", 0.0, NAN);
    kat("log(-0)", 0.0, NAN);
    kat("log(-10)", 0.0, NAN);
    kat("log(x)", NAN, NAN);
    // The constant is Math.log(10.0).
    assert!(same(10f64.ln(), LN_10));
}

#[test]
fn ln_is_math_log_with_jep_s_zero_rules() {
    // NaturalLogarithm.ln@30-47: Math.log for a > 0.
    kat("ln(1)", 0.0, 0.0);
    kat_near("ln(x)", E, 1.0);
    kat_near("ln(10)", 0.0, LN_10);
    kat("ln(x)", INF, INF);
    // @65-80: Complex(a).log() = (Math.log(|a|), atan2(0, a)), real for +0
    // only: ln(+0) = -inf, ln(-0) has imaginary part pi, so NaN.
    kat("ln(0)", 0.0, -INF);
    kat("ln(-0)", 0.0, NAN);
    kat("ln(-1)", 0.0, NAN);
    kat("ln(x)", NAN, NAN);
}

#[test]
fn exp_sqrt_and_abs_are_math() {
    kat("exp(0)", 0.0, 1.0);
    kat_near("exp(1)", 0.0, E);
    kat("exp(x)", -INF, 0.0);
    kat("exp(1000)", 0.0, INF);
    kat("sqrt(16)", 0.0, 4.0);
    kat("sqrt(2)", 0.0, core::f64::consts::SQRT_2);
    kat("sqrt(-0)", 0.0, -0.0);
    // SquareRoot.sqrt@30-47: below zero is complex, so NaN.
    kat("sqrt(-1)", 0.0, NAN);
    kat("abs(-3.5)", 0.0, 3.5);
    kat("abs(3.5)", 0.0, 3.5);
    kat("abs(-0)", 0.0, 0.0);
    kat("abs(x)", -INF, INF);
}

// ---- trigonometry (jep.jar Sine, Cosine, ..., ArcTangent2.run@29-56) ----

#[test]
fn trigonometry_is_math() {
    kat("sin(0)", 0.0, 0.0);
    kat("cos(0)", 0.0, 1.0);
    kat("tan(0)", 0.0, 0.0);
    kat_near("sin(x)", FRAC_PI_2, 1.0);
    kat_near("cos(x)", PI, -1.0);
    kat_near("tan(x)", FRAC_PI_4, 1.0);
    kat_near("asin(1)", 0.0, FRAC_PI_2);
    kat("acos(1)", 0.0, 0.0);
    kat_near("acos(-1)", 0.0, PI);
    kat_near("atan(1)", 0.0, FRAC_PI_4);
    kat("asin(2)", 0.0, NAN);
    kat("acos(-2)", 0.0, NAN);
    kat("sin(x)", INF, NAN);
    // atan2(y, x): the first argument is y.
    kat_near("atan2(1,0)", 0.0, FRAC_PI_2);
    kat("atan2(0,1)", 0.0, 0.0);
    kat_near("atan2(0,-1)", 0.0, PI);
    kat_near("atan2(-1,-1)", 0.0, -3.0 * FRAC_PI_4);
}

// ---- hyperbolic functions: JEP's formulas (jep.jar SineH.sinh@30-51,
// CosineH.cosh@30-51, TanH.tanh@30-64, ArcSineH.asinh@26-54,
// ArcCosineH.acosh@26-60, ArcTanH.atanh@26-69) ----

#[test]
fn hyperbolic_functions_use_jep_s_formulas() {
    // (e - 1/e) / 2, (e + 1/e) / 2.
    kat_near("sinh(1)", 0.0, 1.175_201_193_643_801_4);
    kat_near("cosh(1)", 0.0, 1.543_080_634_815_243_7);
    kat("sinh(0)", 0.0, 0.0);
    kat("cosh(0)", 0.0, 1.0);
    kat_near("sinh(-1)", 0.0, -1.175_201_193_643_801_4);
    // (e^0.5 - e^-0.5) / (e^0.5 + e^-0.5).
    kat_near("tanh(0.5)", 0.0, 0.462_117_157_260_009_8);
    kat("tanh(0)", 0.0, 0.0);
    // inf / inf, as in legacy: not the +-1 of a careful tanh.
    kat("tanh(1000)", 0.0, NAN);
    kat("tanh(-1000)", 0.0, NAN);
    kat("sinh(1000)", 0.0, INF);
    // ln(1 + sqrt(2)), ln(2 + sqrt(3)), ln(3) / 2.
    kat_near("asinh(1)", 0.0, 0.881_373_587_019_543);
    kat("asinh(0)", 0.0, 0.0);
    kat_near("acosh(2)", 0.0, 1.316_957_896_924_816_6);
    kat("acosh(1)", 0.0, 0.0);
    kat_near("atanh(0.5)", 0.0, 0.549_306_144_334_054_9);
    kat_near("atanh(-0.5)", 0.0, -0.549_306_144_334_054_9);
    kat("atanh(0)", 0.0, 0.0);
    // JEP's asinh formula cancels for large negative arguments:
    // -1e10 + sqrt(1e20 + 1) is 0, and ln(0) is -inf.
    kat("asinh(-1e10)", 0.0, -INF);
}

#[test]
fn hyperbolic_inverses_are_nan_outside_their_real_domain() {
    // Complex in JEP (ArcCosineH.acosh@61-83, ArcTanH.atanh@70-86).
    kat("acosh(0.5)", 0.0, NAN);
    kat("acosh(-1)", 0.0, NAN);
    // The formula alone would give ln(-1e10 + 1e10) = -inf here.
    kat("acosh(-1e10)", 0.0, NAN);
    kat("acosh(x)", NAN, NAN);
    kat("atanh(1)", 0.0, NAN);
    kat("atanh(-1)", 0.0, NAN);
    kat("atanh(2)", 0.0, NAN);
    kat("atanh(-2)", 0.0, NAN);
    kat("atanh(x)", NAN, NAN);
}

// ---- binom (jep.jar Binomial.run@95-127; deliberately computed) ----

#[test]
fn binom_is_the_binomial_coefficient() {
    kat("binom(5,2)", 0.0, 10.0);
    kat("binom(5,3)", 0.0, 10.0);
    kat("binom(5,4)", 0.0, 5.0);
    kat("binom(5,0)", 0.0, 1.0);
    kat("binom(5,5)", 0.0, 1.0);
    kat("binom(0,0)", 0.0, 1.0);
    kat("binom(1,1)", 0.0, 1.0);
    kat("binom(10,3)", 0.0, 120.0);
    // C(50, 25) = 126410606437752 (< 2^53, exact).
    kat("binom(50,25)", 0.0, 126_410_606_437_752.0);
    // Arguments are truncated to `int`s (Binomial.run@95-111).
    kat("binom(5.9,2.9)", 0.0, 10.0);
    kat("binom(x,0)", NAN, 1.0);
}

#[test]
fn binom_out_of_range_is_nan_and_huge_is_infinite() {
    // Binomial.run@113-127: n < 0, k < 0 or k > n is an error, so NaN.
    kat("binom(3,4)", 0.0, NAN);
    kat("binom(0,1)", 0.0, NAN);
    kat("binom(-1,0)", 0.0, NAN);
    kat("binom(3,-1)", 0.0, NAN);
    // Overflows to infinity, and stops there rather than looping 2^30 times.
    kat("binom(1100,550)", 0.0, INF);
    kat("binom(2147483647,1073741823)", 0.0, INF);
    // Both saturate to 2147483647 (JLS §5.1.3): C(n, n) = 1.
    kat("binom(1e12,5e11)", 0.0, 1.0);
}

// ---- BitWise (legacy util/BitWise.java:81-105; JLS §5.1.3, §15.19) ----

#[test]
fn bitwise_operations_by_code() {
    // util/BitWise.java:88-100.
    kat("BitWise(24,x,1)", 255.0, 24.0);
    kat("BitWise(24,x,2)", 7.0, 31.0);
    kat("BitWise(24,x,2)", 31.0, 31.0);
    kat("BitWise(24,x,3)", 31.0, 7.0);
    kat("BitWise(4,x,4)", 1.0, 16.0);
    kat("BitWise(4,x,5)", -64.0, -4.0);
    // -64 >>> 4 = 0xFFFFFFC0 >>> 4 = 0x0FFFFFFC.
    kat("BitWise(4,x,6)", -64.0, 268_435_452.0);
    // ~5 = -6; the mask is ignored.
    kat("BitWise(12345,x,7)", 5.0, -6.0);
}

#[test]
fn bitwise_truncates_every_argument_to_an_int() {
    // Double.intValue() (util/BitWise.java:87-100; JLS §5.1.3): toward
    // zero, NaN to 0, saturating at the int range.
    kat("BitWise(255,x,1)", 1.9, 1.0);
    kat("BitWise(255,x,1)", -1.5, 255.0);
    kat("BitWise(255,x,1)", 4_294_967_295.0, 255.0);
    kat("BitWise(255,x,1)", -1e10, 0.0);
    kat("BitWise(255,x,2)", -1e10, -2_147_483_393.0);
    kat("BitWise(255,x,1)", NAN, 0.0);
    kat("BitWise(255,x,1)", INF, 255.0);
    kat("BitWise(2.9,7,1)", 0.0, 2.0);
    kat("BitWise(x,7,1)", NAN, 0.0);
    // The op is truncated too (util/BitWise.java:87).
    kat("BitWise(1,3,x)", 1.9, 1.0);
    kat("BitWise(1,3,x)", 2.5, 3.0);
}

#[test]
fn bitwise_unknown_op_is_zero() {
    // util/BitWise.java:86 `double r = 0`, :102-103 `default: break`.
    kat("BitWise(1,3,x)", 0.0, 0.0);
    kat("BitWise(1,3,x)", 8.0, 0.0);
    kat("BitWise(1,3,x)", -1.0, 0.0);
    kat("BitWise(1,3,x)", NAN, 0.0);
    kat("BitWise(1,3,x)", 0.9, 0.0);
}

#[test]
fn bitwise_shift_distances_use_their_low_five_bits() {
    // JLS §15.19: an int shift uses (distance & 0x1f).
    kat("BitWise(32,1,4)", 0.0, 1.0);
    kat("BitWise(33,1,4)", 0.0, 2.0);
    kat("BitWise(31,1,4)", 0.0, -2_147_483_648.0);
    kat("BitWise(-1,1,4)", 0.0, -2_147_483_648.0);
    kat("BitWise(33,-8,5)", 0.0, -4.0);
    kat("BitWise(x,-8,5)", -31.0, -4.0);
    // -1 >>> 0 is -1 (an int), and -1 >>> 1 is 2^31 - 1.
    kat("BitWise(32,-1,6)", 0.0, -1.0);
    kat("BitWise(1,-1,6)", 0.0, 2_147_483_647.0);
    kat("BitWise(31,-1,6)", 0.0, 1.0);
}

// ---- native & | << >> (module docs, *Integer operations*) ----

#[test]
fn native_bit_operators_are_int_operations() {
    kat("x&255", 4660.0, 52.0);
    kat("x|1", 2.0, 3.0);
    kat("x|3", 5.0, 7.0);
    kat("x&x", -1.0, -1.0);
    kat("1<<x", 4.0, 16.0);
    kat("x>>4", -64.0, -4.0);
    kat("x>>1", 7.0, 3.0);
    kat("1<<31", 0.0, -2_147_483_648.0);
    kat("1<<x", 32.0, 1.0);
    kat("1<<x", -1.0, -2_147_483_648.0);
    kat("x>>x", 33.0, 16.0);
    kat("5.9&3", 0.0, 1.0);
    kat("x&255", 1e10, 255.0);
    kat("x<<1", NAN, 0.0);
    kat("x|0", -2.5, -2.0);
    // `&` binds tighter than `==`: (4 & 4) == 4.
    kat("x&4==4", 4.0, 1.0);
}

proptest! {
    /// `a & b`, `a | b`, `a << b`, `a >> b` are `BitWise(b, a, 1/2/4/5)`.
    #[test]
    fn native_operators_match_bitwise(a in any_number(), b in any_number()) {
        for (op, code) in [("&", 1), ("|", 2), ("<<", 4), (">>", 5)] {
            let native = ok(&format!("P1{op}P2")).eval(0.0, &[a, b]);
            // params() is in order of first appearance: P2, then P1.
            let bitwise = ok(&format!("BitWise(P2,P1,{code})")).eval(0.0, &[b, a]);
            prop_assert_eq!(native, bitwise);
        }
    }
}

fn any_number() -> impl Strategy<Value = f64> {
    prop_oneof![
        any::<f64>(),
        (-70i32..70).prop_map(f64::from),
        any::<i32>().prop_map(f64::from),
        Just(f64::NAN),
        Just(4_294_967_296.0),
    ]
}

// ---- results and bindings ----

#[test]
fn non_finite_results_are_errors_carrying_the_value() {
    let e = ok("1/x");
    assert_eq!(e.eval(2.0, &[]), Ok(0.5));
    assert_eq!(e.eval(0.0, &[]), Err(EvalError::NonFinite(INF)));
    assert_eq!(e.eval(-0.0, &[]), Err(EvalError::NonFinite(-INF)));
    match ok("x/x").eval(0.0, &[]) {
        Err(EvalError::NonFinite(v)) => assert!(v.is_nan()),
        other => panic!("{other:?}"),
    }
    // -0 is finite.
    assert_eq!(
        ok("-x").eval(0.0, &[]).map(f64::to_bits),
        Ok((-0.0f64).to_bits())
    );
    assert_eq!(ok("x").eval(f64::MAX, &[]), Ok(f64::MAX));
}

#[test]
fn parameters_bind_by_param_id() {
    // params() is P2, [P1:a] (first appearance), so values go in that order.
    let e = ok("P2-[P1:a]+P2");
    assert_eq!(
        e.params(),
        [ParamRef::new("P2", None), ParamRef::new("P1", Some("a"))]
    );
    assert_eq!(e.eval(NAN, &[10.0, 3.0]), Ok(17.0));
    assert_eq!(e.eval(NAN, &[3.0, 10.0]), Ok(-4.0));
    // `x` and parameters together.
    assert_eq!(ok("x*P1").eval(3.0, &[4.0]), Ok(12.0));
}

#[test]
fn wrong_number_of_values_is_a_binding_error() {
    let e = ok("[P1:a]+P2");
    let err = |found| Err(EvalError::Bindings { expected: 2, found });
    assert_eq!(e.eval(0.0, &[1.0]), err(1));
    assert_eq!(e.eval(0.0, &[]), err(0));
    assert_eq!(e.eval(0.0, &[1.0, 2.0, 3.0]), err(3));
    assert_eq!(e.eval_with(&mut EvalStack::new(), 0.0, &[1.0]), err(1));
    assert_eq!(e.eval(0.0, &[1.0, 2.0]), Ok(3.0));
    assert_eq!(
        ok("x").eval(0.0, &[1.0]),
        Err(EvalError::Bindings {
            expected: 0,
            found: 1
        })
    );
    // A deep formula checks too.
    let deep = deep_sum(40, "P1");
    assert_eq!(
        deep.eval(0.0, &[]),
        Err(EvalError::Bindings {
            expected: 1,
            found: 0
        })
    );
}

#[test]
fn errors_display_and_are_errors() {
    let b = EvalError::Bindings {
        expected: 2,
        found: 1,
    };
    assert_eq!(
        b.to_string(),
        "the formula has 2 parameter reference(s) but 1 value(s) were given"
    );
    assert_eq!(
        EvalError::NonFinite(INF).to_string(),
        "the formula's result is inf, not a finite number"
    );
    assert_eq!(
        EvalError::NonFinite(NAN).to_string(),
        "the formula's result is NaN, not a finite number"
    );
    let _: &dyn core::error::Error = &b;
}

// ---- the stack ----

/// `sum(1, 2, ..., n)` with the last argument replaced by `last`, parsed
/// with the argument and stack limits lifted (deeper than
/// [`INLINE_STACK`] needs them raised: US-RR-20-8).
fn deep_sum(n: usize, last: &str) -> Expr {
    let mut args: Vec<String> = (1..n).map(|i| i.to_string()).collect();
    args.push(last.to_owned());
    let src = format!("sum({})", args.join(","));
    match crate::expr::parse_with(&src, ROOMY) {
        Ok(e) => e,
        Err(err) => panic!("{src:?} should parse, got {err}"),
    }
}

#[test]
fn stack_depth_is_the_most_values_held_at_once() {
    for (src, depth) in [
        ("x", 1),
        ("-x", 1),
        ("1+2", 2),
        ("1+2+3", 2),
        ("1+(2+3)", 3),
        ("1+(2+(3+4))", 4),
        ("2^3^2", 3),
        ("-(-(-x))", 1),
        ("sum(1,2,3,4)", 4),
        ("sum(1,2)*3", 2),
        ("1*sum(1,2,3)", 4),
        ("if(x>1,x,0)", 3),
        ("BitWise(24,x,1)", 3),
        ("[P1:a]-[P2:b]", 2),
    ] {
        assert_eq!(ok(src).stack_depth(), depth, "{src:?}");
    }
    assert_eq!(deep_sum(40, "40").stack_depth(), 40);
}

#[test]
fn formulas_deeper_than_the_inline_stack_still_evaluate() {
    for n in [INLINE_STACK - 1, INLINE_STACK, INLINE_STACK + 1, 100] {
        let e = deep_sum(n, &n.to_string());
        assert_eq!(e.stack_depth(), n);
        let expected = (n * (n + 1) / 2) as f64;
        assert_eq!(e.eval(0.0, &[]), Ok(expected), "n={n}");
        let mut stack = EvalStack::new();
        assert_eq!(e.eval_with(&mut stack, 0.0, &[]), Ok(expected), "n={n}");
        assert_eq!(stack.capacity(), n);
    }
    let e = deep_sum(40, "P1");
    assert_eq!(e.eval(0.0, &[2.0]), Ok(782.0));
}

#[test]
fn an_eval_stack_grows_once_and_is_reused() {
    let mut stack = EvalStack::new();
    assert_eq!(stack.capacity(), 0);
    assert_eq!(EvalStack::default().capacity(), 0);
    let small = ok("(x-32)/1.8");
    let big = deep_sum(50, "50");
    assert_eq!(EvalStack::for_expr(&big).capacity(), 50);
    assert_eq!(small.eval_with(&mut stack, 212.0, &[]), Ok(100.0));
    assert_eq!(stack.capacity(), 2);
    assert_eq!(big.eval_with(&mut stack, 0.0, &[]), Ok(1275.0));
    assert_eq!(stack.capacity(), 50);
    // Leftover values from the big formula do not leak into the small one,
    // and the stack does not shrink.
    assert_eq!(small.eval_with(&mut stack, 32.0, &[]), Ok(0.0));
    assert_eq!(stack.capacity(), 50);
    let mut sized = EvalStack::for_expr(&small);
    assert_eq!(small.eval_with(&mut sized, 50.0, &[]), Ok(10.0));
    assert_eq!(sized.capacity(), 2);
}

#[test]
fn operands_are_taken_in_order() {
    // Non-commutative operators and calls see their operands left to right.
    kat("10-3", 0.0, 7.0);
    kat("12/4", 0.0, 3.0);
    kat("2^3", 0.0, 8.0);
    kat("7%4", 0.0, 3.0);
    kat("(1+1)-(2*3)", 0.0, -4.0);
    kat("atan2(1,1/0)", 0.0, 0.0);
    kat("if(0,1,2)-if(1,10,20)", 0.0, -8.0);
    // mask 1, value 6, op 3: 6 ^ 1.
    kat("BitWise(x+1,(x+3)*2,x+3)", 0.0, 7.0);
    kat("binom(6,2)-binom(2,1)", 0.0, 13.0);
}

// ---- threads ----

#[test]
fn expressions_are_send_and_sync_and_evaluate_concurrently() {
    const fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<Expr>();
    assert_send_sync::<EvalError>();
    assert_send_sync::<EvalStack>();

    let e = ok("if(x>32767,x-65536,x)*[P1:a]");
    std::thread::scope(|s| {
        let handles: Vec<_> = (0..4)
            .map(|t| {
                let e = &e;
                s.spawn(move || {
                    let mut stack = EvalStack::for_expr(e);
                    let mut total = 0.0;
                    for i in 0..10_000u32 {
                        let x = f64::from(i * 7 + t);
                        total += e.eval_with(&mut stack, x, &[0.5]).unwrap();
                    }
                    total
                })
            })
            .collect();
        let totals: Vec<f64> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        for (t, total) in totals.into_iter().enumerate() {
            let t = u32::try_from(t).unwrap();
            let expected: f64 = (0..10_000u32)
                .map(|i| {
                    let x = f64::from(i * 7 + t);
                    (if x > 32767.0 { x - 65536.0 } else { x }) * 0.5
                })
                .sum();
            assert!(same(total, expected), "thread {t}");
        }
    });
}

// ---- corpus formulas (D-028 pinned corpus; parser tests' CORPUS_SAMPLE) ----

#[test]
fn corpus_formulas_known_answers() {
    // ROM scalings.
    kat("(x-32)/1.8", 212.0, 100.0);
    kat("(x*1.8)+32", 100.0, 212.0);
    kat("(x*.3515625)-20", 128.0, 25.0);
    kat("(x+20)/.3515625", 25.0, 128.0);
    kat("14.7/(1+x*.0078125)", 0.0, 14.7);
    kat("14.7/(1+x*.0078125)", 128.0, 7.35);
    kat("(14.7/x-1)/.0078125", 7.35, 128.0);
    kat("x*3.515625e-1+-4.5e1", 128.0, 0.0);
    kat("(x--4.5e1)/3.515625e-1", 0.0, 128.0);
    kat("x/4.8828125e-4", 1.0, 2048.0);
    kat("x*1.0e-2+0.0", 250.0, 2.5);
    kat("-x", 5.0, -5.0);
    kat("96560.6/x", 0.0, INF);
    // if / % forms from the corpus.
    kat("if(x%256==255,0,((x%256)-128)*100/128)", 255.0, 0.0);
    kat("if(x%256==255,0,((x%256)-128)*100/128)", 448.0, 50.0);
    kat("if(x%256==255,0,((x%256)-128)*100/128)", 0.0, -100.0);
    kat(
        "if(x<2147483647,((x+2147483648)%65536)/8192,(x%65536)/8192)",
        -1.0,
        7.999_877_929_687_5,
    );
    kat(
        "if(x<2147483647,((x+2147483648)%65536)/8192,(x%65536)/8192)",
        2_147_483_648.0,
        0.0,
    );
    kat(
        "if(256+x/16777216>127,(256+x/16777216)-128,x/16777216)",
        16_777_216.0,
        129.0,
    );
    // Derived logger parameters.
    let d = ok("[P7:psi]-[P24:psi]");
    assert_eq!(d.eval(NAN, &[30.5, 14.5]), Ok(16.0));
    let d = ok("P160/[P7:hPa]");
    assert_eq!(d.eval(NAN, &[500.0, 1000.0]), Ok(0.5));
    let d = ok("(P8*[P21:ms])/1200");
    assert_eq!(d.eval(NAN, &[3000.0, 4.0]), Ok(10.0));
    let d = ok("P156*(835-(0.7*([P31:C]-15)))/1000");
    // 2 * (835 - 0.7 * (25 - 15)) / 1000 = 2 * 828 / 1000.
    assert_eq!(d.eval(NAN, &[2.0, 25.0]), Ok(1.656));
}

#[test]
fn every_corpus_sample_formula_evaluates() {
    for &src in CORPUS_SAMPLE {
        let e = ok(src);
        let params = vec![1.5; e.params().len()];
        let v = raw(e.eval(100.0, &params));
        assert!(v.is_finite(), "{src:?} gave {v:?}");
        assert!(e.stack_depth() <= INLINE_STACK, "{src:?}");
    }
}

// ---- property tests ----

/// Evaluates the generated tree directly, by recursion, with the same
/// scalar rules: an oracle for the stack machine's operand order and
/// bookkeeping.
fn reference(t: &Tree, x: f64, e: &Expr, params: &[f64]) -> f64 {
    match t {
        Tree::Num(v) => *v,
        Tree::X => x,
        Tree::Param(id, unit) => {
            let i = e
                .params()
                .iter()
                .position(|p| p.id() == *id && p.unit() == *unit)
                .unwrap();
            params[i]
        }
        Tree::Unary(op, a) => unary(*op, reference(a, x, e, params)),
        Tree::Binary(op, a, b) => {
            binary(*op, reference(a, x, e, params), reference(b, x, e, params))
        }
        Tree::Call(f, args) => {
            let vals: Vec<f64> = args.iter().map(|a| reference(a, x, e, params)).collect();
            call(*f, &vals)
        }
    }
}

fn input() -> impl Strategy<Value = f64> {
    prop_oneof![
        -1e6..1e6f64,
        (-300i32..300).prop_map(f64::from),
        any::<f64>(),
        Just(0.0),
        Just(-0.0),
    ]
}

fn params_for(e: &Expr, seed: f64) -> Vec<f64> {
    (0..e.params().len())
        .map(|i| seed * f64::from(u32::try_from(i).unwrap() + 1) - 3.0)
        .collect()
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(1024))]

    /// The stack machine agrees with direct recursive evaluation.
    #[test]
    fn stack_machine_matches_recursive_evaluation(t in any_tree(), x in input(), seed in input()) {
        let e = ok(&t.source());
        let params = params_for(&e, seed);
        let got = raw(e.eval(x, &params));
        let want = reference(&t, x, &e, &params);
        prop_assert!(same(got, want), "{}: got {got:?}, want {want:?}", t.source());
    }

    /// Evaluation is deterministic, `eval` and `eval_with` agree, and the
    /// printed formula evaluates to the same value as the original.
    #[test]
    fn evaluation_is_deterministic_and_survives_printing(
        t in any_tree(),
        x in input(),
        seed in input(),
    ) {
        let e = ok(&t.source());
        let params = params_for(&e, seed);
        let first = raw(e.eval(x, &params));
        prop_assert!(same(raw(e.eval(x, &params)), first));
        let mut stack = EvalStack::new();
        prop_assert!(same(raw(e.eval_with(&mut stack, x, &params)), first));
        prop_assert!(same(raw(e.eval_with(&mut stack, x, &params)), first));
        let printed = ok(&e.to_string());
        prop_assert_eq!(printed.params(), e.params());
        prop_assert!(same(raw(printed.eval(x, &params)), first));
    }

    /// The result is `Ok` exactly when it is finite.
    #[test]
    fn ok_exactly_when_finite(t in any_tree(), x in input()) {
        let e = ok(&t.source());
        let params = params_for(&e, 2.0);
        match e.eval(x, &params) {
            Ok(v) => prop_assert!(v.is_finite()),
            Err(EvalError::NonFinite(v)) => prop_assert!(!v.is_finite()),
            Err(other) => prop_assert!(false, "{other:?}"),
        }
    }
}

#[test]
fn node_ids_are_not_needed_to_evaluate() {
    // The stack machine reads nodes in order; ids only document the tree.
    let e = ok("(1+2)*(3+4)");
    assert_eq!(e.root(), NodeId::new(6));
    assert_eq!(e.eval(0.0, &[]), Ok(21.0));
}
