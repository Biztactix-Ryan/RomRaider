//! Evaluation of a parsed [`Expr`] (US-RR-20-7, MF-25).
//!
//! [`Expr::eval`] walks the post-order arena once, front to back, with a
//! value stack: a leaf pushes its value, an operator or call replaces its
//! operands (the top of the stack, in order) with its result, and the root
//! leaves the one value that is the answer. How deep that stack gets is
//! fixed when the formula is parsed ([`Expr::stack_depth`]), so evaluation
//! never grows anything:
//!
//! * [`Expr::eval`] keeps the stack in an inline `[f64; INLINE_STACK]` on the
//!   native stack whenever the formula fits; it allocates nothing, locks
//!   nothing and looks nothing up. Every formula [`parse`](super::parse)
//!   accepts fits: its evaluation stack limit,
//!   [`Limits::max_stack`](super::Limits::max_stack), defaults to
//!   `INLINE_STACK` (US-RR-20-8), which every formula in the pinned corpus
//!   is within by a wide margin. Only a formula parsed with a raised limit
//!   ([`parse_with`](super::parse_with)) can be deeper, and `eval` gives it
//!   a heap buffer for that call;
//! * [`Expr::eval_with`] takes the stack from the caller, as an
//!   [`EvalStack`] it keeps between calls ([`EvalStack::for_expr`] sizes it
//!   once, at bind time), and never allocates once the stack is big enough.
//!
//! An `Expr` is immutable and holds no cell, lock or cache, so it is
//! `Send + Sync` and any number of threads can evaluate one at once. That
//! is MF-25: legacy serialised every evaluation in the program on one lock
//! around one global formula cache (`util/JEPUtil.java:44`, `:46`, `:62`).
//! `cargo xtask check-globals` keeps it so: no source in this module may
//! name a lock, cell, atomic or lazy/once type at all, since a `Mutex` field
//! would leave `Expr` `Send + Sync`. The proofs are
//! `tests/expr_eval_no_alloc.rs` (no allocation, alone or on many threads
//! sharing one `&Expr`) and the throughput and thread scaling measured by
//! `benches/expr_eval.rs` (US-RR-20-4).
//!
//! # Binding
//!
//! `x` is passed by value. The parameter references are passed as a slice
//! with one value per [`Expr::params`] entry, in that order, so the value of
//! [`ParamId`](super::ParamId) `p` is `params[p.index()]`. A caller resolves
//! each reference to its source once, when it binds the formula, and per
//! sample only fills a reusable `[f64]`: no string replacement, map or
//! hashing per evaluation, which legacy did on every sample
//! (`logger/ecu/definition/EcuDerivedParameterConvertorImpl.java:53-66`).
//!
//! # Numeric semantics
//!
//! Values are `f64`, as in JEP, and every operation follows the JEP 2.x
//! class legacy ran (`legacy/lib/common/jep.jar`, read as bytecode, never
//! run, D-024) with Java's `double` arithmetic. Cited below as
//! `Class.method@offsets`, the bytecode offsets in the jar's
//! `org/nfunk/jep/function/` (or `org/nfunk/jep/`) class file.
//!
//! * `+ - * /`, prefix `-`: IEEE 754, so `1/0` is infinity and `0/0` NaN
//!   (`Add`, `Subtract`, `Multiply`, `Divide`, `UMinus`: one `dadd`, `dsub`,
//!   `dmul`, `ddiv`, `dneg` on the two `doubleValue`s).
//! * `%` and `mod`: Java's `%` on doubles (`drem`, `Modulus.run@47-52`):
//!   the remainder has the sign of the dividend, `a % 0` is NaN (JLS
//!   §15.17.3). Rust's `%` on `f64` is the same operation.
//! * `^` and `pow`: `Math.pow`, except that a negative base with an exponent
//!   that is not an `int` makes a complex number (`Power.power(Number,
//!   Number)@0-43`), which is NaN here (see *Complex results*). `Math.pow`
//!   differs from C's `pow` in two cases, both NaN in Java: an exponent of
//!   NaN (`1^NaN`), and a base of `±1` with an infinite exponent.
//! * comparisons give `1` or `0` (`Comparative.run@123-157`); IEEE, so any
//!   comparison with NaN is `0` except `!=`, which is `1`
//!   (`Comparative.lt`/`gt`/`le`/`ge`/`eq`/`ne`: `dcmpg`/`dcmpl` on the two
//!   doubles).
//! * `&&`, `||`, `!`: an operand is true when it is not `0`, so NaN is true
//!   (`Logical.run@76-121`, `Not.run@17-34`).
//! * `if(c, a, b)` is `a` when `c > 0`, else `b`; `if(c, a, b, d)` is `a` when
//!   `c > 0`, `b` when `c < 0`, else (zero or NaN) `d` (`If.evaluate@139-199`).
//!   JEP evaluates only the branch it takes; here every argument is
//!   evaluated, which gives the same value because evaluation is pure and
//!   cannot fail part-way.
//! * `sum(a, b, c)` is `a + (b + c)`: JEP adds from the last argument back
//!   (`Sum.run@23-58`), which matters for rounding.
//! * `log(a)` is `ln(a) / ln(10)`, not a correctly rounded base-10 log, for
//!   `a > 0` (`Logarithm.log@36-57`, `LOG10 = Math.log(10.0)` in
//!   `Logarithm.<clinit>`), so `log(1000)` is `2.9999999999999996`; `log(0)`
//!   is NaN, not `-inf` (the complex branch divides `(-inf, 0)` by
//!   `(ln 10, 0)`, whose imaginary part is `0 - 0 * -inf` = NaN:
//!   `Complex.div@0-76`).
//! * `ln(a)`: `Math.log(a)` for `a > 0` (`NaturalLogarithm.ln@30-47`), `-inf`
//!   for `+0` and NaN for `-0` (the complex logarithm of `±0` is
//!   `(-inf, atan2(0, ±0))`, real only for `+0`: `Complex.log`,
//!   `Complex.arg`), NaN below zero.
//! * `sqrt(a)`: `Math.sqrt`, NaN below zero (`SquareRoot.sqrt@30-59`).
//! * `sinh`, `cosh`, `tanh`, `asinh`, `acosh`, `atanh`: JEP's own formulas,
//!   not the library functions, so results match legacy and share its
//!   overflow (`tanh(1000)` is `inf/inf` = NaN): `(e^a - e^-a) / 2`
//!   (`SineH.sinh@30-51`), `(e^a + e^-a) / 2` (`CosineH.cosh@30-51`),
//!   `(exp(a) - exp(-a)) / (pow(E, a) + pow(E, -a))` (`TanH.tanh@30-64`),
//!   `ln(a + sqrt(a*a + 1))` (`ArcSineH.asinh@26-54`),
//!   `ln(a + sqrt(a*a - 1))` for `a >= 1` (`ArcCosineH.acosh@26-60`) and
//!   `ln((1 + a) / (1 - a)) / 2` for `-1 < a < 1` (`ArcTanH.atanh@26-69`);
//!   NaN outside those ranges (complex in JEP).
//! * `sin`, `cos`, `tan`, `asin`, `acos`, `atan`, `atan2(y, x)`, `exp`,
//!   `abs`: the `java.lang.Math` function of the same name (`Sine.sin`,
//!   ..., `ArcTangent2.run@29-56`). Java lets `Math` transcendental functions
//!   be off by one ulp, so legacy itself was not bit-identical across JVMs;
//!   these use Rust's `f64` methods, with the same special values.
//! * `binom(n, k)`: the binomial coefficient, `n` and `k` truncated to `int`
//!   as JEP does (`Binomial.run@95-111`), NaN when `n < 0`, `k < 0` or
//!   `k > n` (`Binomial.run@113-127`). **Deliberately not legacy**: the
//!   `Binomial` in legacy's jar never returns a value: its `<clinit>` makes
//!   `coeffs = new int[21][]` with every row null and `expand` never fills
//!   rows 0 to 20, so every call throws `NullPointerException`, which
//!   `JEP.getValueAsObject` catches (its handler for `RuntimeException`), so
//!   legacy's value was always NaN. No working definition can call it; here
//!   it computes what JEP documents, in `f64` (exact below 2^53, infinity
//!   once it overflows, which also bounds the work).
//! * `BitWise(mask, value, op)`: see *Integer operations*.
//!
//! # Integer operations
//!
//! `BitWise` and the native `& | << >>` work on Java `int`s, as legacy's
//! `BitWise` does (`util/BitWise.java:87-101`):
//!
//! * each operand becomes an `int` by Java's `double`-to-`int` conversion
//!   (`Double.intValue()`, JLS §5.1.3): truncation toward zero, NaN to `0`,
//!   and values beyond the `int` range saturated to `i32::MIN` / `i32::MAX`
//!   (so `4294967295 & 255` is `2147483647 & 255` = 255). Rust's `f64 as
//!   i32` is that same conversion;
//! * a shift uses only the low five bits of its distance, as Java's `<<`,
//!   `>>` and `>>>` on `int` do (JLS §15.19): `1 << 32` is `1`, `1 << -1` is
//!   `1 << 31` = `-2147483648`. Shifts by 32 or more, or by a negative
//!   distance, are therefore not refused; they mean what they meant in
//!   legacy;
//! * the result is the `int` as a double, so it is always in the `int`
//!   range, and `>>>` (code 6) can give a negative result when its distance
//!   is a multiple of 32.
//!
//! `x & m`, `x | m`, `x << m` and `x >> m` are exactly `BitWise(m, x, 1)`,
//! `BitWise(m, x, 2)`, `BitWise(m, x, 4)` and `BitWise(m, x, 5)`. The `op` of
//! `BitWise` is itself truncated to an `int` (`util/BitWise.java:87`), so a
//! computed `op` of `1.9` is `1`, and an `op` that names no operation gives
//! `0` (`util/BitWise.java:86`, `:102-103`); a literal `op` that names none
//! is refused when the formula is parsed.
//!
//! # Complex results
//!
//! JEP carries complex numbers: a negative base to a non-`int` power, or the
//! logarithm, square root, `acosh` or `atanh` of a value outside its real
//! domain, gives a `Complex` that flows into the rest of the formula, and
//! `JEP.getValue` (`@13-42`) returns NaN for a final value with a non-zero
//! imaginary part. Here each such value is NaN at once. The two differ only
//! for a formula that does something further with a complex value and gets
//! back to the real line (`log(-1)*0` is `0` in JEP), or compares one (an
//! error, so NaN, in JEP; `0` here, since a NaN comparison is false). No
//! definition does either.
//!
//! # Non-finite results
//!
//! Intermediate values may be infinite or NaN; only the final value is
//! checked. A final NaN or infinity is [`EvalError::NonFinite`], carrying the
//! value. Legacy passed it on: the ROM editor displayed it
//! (`maps/DataCell.java:425`, `util/NumberUtil.java:65`) and every logger
//! convertor turned it into `0.0` without a word
//! (`logger/ecu/definition/EcuParameterConvertorImpl.java:113`,
//! `EcuDerivedParameterConvertorImpl.java:67`,
//! `ExternalDataConvertorImpl.java:50`). Making it an error lets each caller
//! choose, and the carried value lets one that wants legacy's behaviour have
//! it.

use core::f64::consts::{E, LN_10};
use core::fmt;

use super::ast::{BinaryOp, Expr, Func, NodeKind, UnaryOp};

/// How deep a formula's value stack ([`Expr::stack_depth`]) may be for
/// [`Expr::eval`] to keep it inline, on the native stack. It is also the
/// default [`Limits::max_stack`](super::Limits::max_stack), so every formula
/// [`parse`](super::parse) accepts is within it. Deeper formulas, parsed
/// with raised limits, still evaluate; `eval` gives them a heap buffer, and
/// [`Expr::eval_with`] a caller-owned one.
pub const INLINE_STACK: usize = 32;

/// Why [`Expr::eval`] gave no value.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum EvalError {
    /// The parameter values do not match [`Expr::params`]: there must be
    /// exactly one per reference.
    Bindings {
        /// `params().len()`.
        expected: usize,
        /// The number of values given.
        found: usize,
    },
    /// The result is NaN or infinite; the value is carried (see the
    /// [module docs](self#non-finite-results)).
    NonFinite(f64),
}

impl fmt::Display for EvalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bindings { expected, found } => write!(
                f,
                "the formula has {expected} parameter reference(s) but {found} value(s) were given"
            ),
            Self::NonFinite(v) => write!(f, "the formula's result is {v}, not a finite number"),
        }
    }
}

impl core::error::Error for EvalError {}

/// A value stack for [`Expr::eval_with`], kept by the caller and reused.
///
/// It grows to the [`stack_depth`](Expr::stack_depth) of the deepest formula
/// it is used with and never shrinks, so after the first use (or from the
/// start, with [`for_expr`](Self::for_expr)) evaluating allocates nothing
/// whatever the formula's size. It holds no result between calls; one stack
/// serves any number of formulas, one evaluation at a time.
#[derive(Debug, Clone)]
pub struct EvalStack {
    slots: Vec<f64>,
}

/// [`EvalStack::new`].
impl Default for EvalStack {
    fn default() -> Self {
        Self::new()
    }
}

impl EvalStack {
    /// An empty stack; the first evaluation sizes it.
    #[must_use]
    pub const fn new() -> Self {
        Self { slots: Vec::new() }
    }

    /// A stack already big enough for `expr`.
    #[must_use]
    pub fn for_expr(expr: &Expr) -> Self {
        Self {
            slots: vec![0.0; expr.stack_depth()],
        }
    }

    /// How many values it holds room for.
    #[must_use]
    pub fn capacity(&self) -> usize {
        self.slots.len()
    }
}

impl Expr {
    /// Evaluates the formula with `x` as the converted value and `params`
    /// as the values of [`params`](Self::params), in order.
    ///
    /// Allocates nothing and takes no lock when
    /// [`stack_depth`](Self::stack_depth) is at most [`INLINE_STACK`], as it
    /// is for every formula parsed within [`Limits::DEFAULT`](super::Limits::DEFAULT);
    /// [`eval_with`](Self::eval_with) never allocates whatever the depth.
    /// The numeric rules are in the [module docs](super::eval).
    ///
    /// # Errors
    ///
    /// [`EvalError::Bindings`] when `params.len()` is not
    /// `self.params().len()`, and [`EvalError::NonFinite`] when the result is
    /// NaN or infinite.
    pub fn eval(&self, x: f64, params: &[f64]) -> Result<f64, EvalError> {
        if self.stack_depth() > INLINE_STACK {
            return self.eval_with(&mut EvalStack::for_expr(self), x, params);
        }
        self.check_bindings(params)?;
        let mut slots = [0.0; INLINE_STACK];
        finite(self.run(&mut slots, x, params))
    }

    /// As [`eval`](Self::eval), with the value stack supplied by the caller.
    /// `stack` grows if it is too small for this formula, and otherwise is
    /// only written to.
    ///
    /// # Errors
    ///
    /// As [`eval`](Self::eval).
    pub fn eval_with(
        &self,
        stack: &mut EvalStack,
        x: f64,
        params: &[f64],
    ) -> Result<f64, EvalError> {
        self.check_bindings(params)?;
        let depth = stack.slots.len().max(self.stack_depth());
        stack.slots.resize(depth, 0.0);
        finite(self.run(&mut stack.slots, x, params))
    }

    fn check_bindings(&self, params: &[f64]) -> Result<(), EvalError> {
        let expected = self.params().len();
        if params.len() == expected {
            Ok(())
        } else {
            Err(EvalError::Bindings {
                expected,
                found: params.len(),
            })
        }
    }

    /// The stack machine. `slots` holds at least `stack_depth()` values and
    /// `params` one per reference; given those, every index below is in
    /// range (the parser builds the nodes in post-order, so each node's
    /// operands are the top of the stack). Reads outside them would give
    /// NaN and writes would be dropped, rather than panic.
    fn run(&self, slots: &mut [f64], x: f64, params: &[f64]) -> f64 {
        let at = |slots: &[f64], i: usize| slots.get(i).copied().unwrap_or(f64::NAN);
        // Number of values on the stack.
        let mut len: usize = 0;
        for node in self.nodes() {
            // `base` is where the node's value goes: the slot of its first
            // operand, or the next free slot for a leaf.
            let (base, value) = match *node.kind() {
                NodeKind::Number(v) => (len, v),
                NodeKind::X => (len, x),
                NodeKind::Param(p) => (len, params.get(p.index()).copied().unwrap_or(f64::NAN)),
                NodeKind::Unary { op, .. } => {
                    let base = len.saturating_sub(1);
                    (base, unary(op, at(slots, base)))
                }
                NodeKind::Binary { op, .. } => {
                    let base = len.saturating_sub(2);
                    let (a, b) = (at(slots, base), at(slots, base + 1));
                    (base, binary(op, a, b))
                }
                NodeKind::Call { func, args } => {
                    let base = len.saturating_sub(args.len());
                    (base, call(func, slots.get(base..len).unwrap_or(&[])))
                }
            };
            if let Some(slot) = slots.get_mut(base) {
                *slot = value;
            }
            len = base + 1;
        }
        at(slots, 0)
    }
}

fn finite(v: f64) -> Result<f64, EvalError> {
    if v.is_finite() {
        Ok(v)
    } else {
        Err(EvalError::NonFinite(v))
    }
}

/// `1` for true, `0` for false (`Comparative.run@123-157`).
fn truth(b: bool) -> f64 {
    if b { 1.0 } else { 0.0 }
}

fn unary(op: UnaryOp, a: f64) -> f64 {
    match op {
        UnaryOp::Neg => -a,
        UnaryOp::Not => truth(a == 0.0),
    }
}

fn binary(op: BinaryOp, a: f64, b: f64) -> f64 {
    match op {
        BinaryOp::Or => truth(a != 0.0 || b != 0.0),
        BinaryOp::And => truth(a != 0.0 && b != 0.0),
        BinaryOp::Eq => truth(a == b),
        BinaryOp::Ne => truth(a != b),
        BinaryOp::Lt => truth(a < b),
        BinaryOp::Gt => truth(a > b),
        BinaryOp::Le => truth(a <= b),
        BinaryOp::Ge => truth(a >= b),
        BinaryOp::BitOr => int_op(2, b, a),
        BinaryOp::BitAnd => int_op(1, b, a),
        BinaryOp::Shl => int_op(4, b, a),
        BinaryOp::Shr => int_op(5, b, a),
        BinaryOp::Add => a + b,
        BinaryOp::Sub => a - b,
        BinaryOp::Mul => a * b,
        BinaryOp::Div => a / b,
        BinaryOp::Mod => a % b,
        BinaryOp::Pow => pow(a, b),
    }
}

fn call(func: Func, args: &[f64]) -> f64 {
    let arg = |i: usize| args.get(i).copied().unwrap_or(f64::NAN);
    let a = arg(0);
    match func {
        Func::Sin => a.sin(),
        Func::Cos => a.cos(),
        Func::Tan => a.tan(),
        Func::Asin => a.asin(),
        Func::Acos => a.acos(),
        Func::Atan => a.atan(),
        Func::Atan2 => a.atan2(arg(1)),
        Func::Sinh => (a.exp() - (-a).exp()) / 2.0,
        Func::Cosh => (a.exp() + (-a).exp()) / 2.0,
        Func::Tanh => (a.exp() - (-a).exp()) / (E.powf(a) + E.powf(-a)),
        Func::Asinh => (a + (a * a + 1.0).sqrt()).ln(),
        Func::Acosh => {
            if a >= 1.0 {
                (a + (a * a - 1.0).sqrt()).ln()
            } else {
                f64::NAN
            }
        }
        Func::Atanh => {
            if a > -1.0 && a < 1.0 {
                ((1.0 + a) / (1.0 - a)).ln() / 2.0
            } else {
                f64::NAN
            }
        }
        Func::Log => {
            if a > 0.0 {
                a.ln() / LN_10
            } else {
                f64::NAN
            }
        }
        Func::Ln => {
            if a > 0.0 || (a == 0.0 && a.is_sign_positive()) {
                a.ln()
            } else {
                f64::NAN
            }
        }
        Func::Exp => a.exp(),
        Func::Pow => pow(a, arg(1)),
        Func::Sqrt => a.sqrt(),
        Func::Abs => a.abs(),
        Func::Mod => a % arg(1),
        // From the last argument back: `a + (b + c)` (`Sum.run@23-58`).
        Func::Sum => args
            .iter()
            .rev()
            .copied()
            .reduce(|acc, v| v + acc)
            .unwrap_or(f64::NAN),
        Func::If => {
            if a > 0.0 {
                arg(1)
            } else if args.len() == 3 || a < 0.0 {
                arg(2)
            } else {
                arg(3)
            }
        }
        Func::Binom => binom(a, arg(1)),
        Func::BitWise => int_op(java_int(arg(2)), a, arg(1)),
    }
}

/// JEP's `^` (`Power.power(Number, Number)`) with `Math.pow`'s special
/// cases.
fn pow(a: f64, b: f64) -> f64 {
    // A negative base and an exponent that is not an `int`: complex in JEP.
    if a < 0.0 && b != f64::from(java_int(b)) {
        return f64::NAN;
    }
    // Where `Math.pow` and C's `pow` disagree (Math.pow javadoc): NaN
    // exponent, and |base| = 1 with an infinite exponent, are NaN in Java.
    if b.is_nan() || (a.abs() == 1.0 && b.is_infinite()) {
        return f64::NAN;
    }
    a.powf(b)
}

/// `Double.intValue()` (JLS §5.1.3): toward zero, NaN to 0, saturating,
/// which is exactly what `as` does.
const fn java_int(v: f64) -> i32 {
    v as i32
}

/// `BitWise(mask, value, op)` on Java `int`s (`util/BitWise.java:86-105`).
/// Shift distances use their low five bits (JLS §15.19), which
/// `wrapping_shl` / `wrapping_shr` do too.
fn int_op(op: i32, mask: f64, value: f64) -> f64 {
    let m = java_int(mask);
    let v = java_int(value);
    let shift = m.cast_unsigned();
    let r = match op {
        1 => v & m,
        2 => v | m,
        3 => v ^ m,
        4 => v.wrapping_shl(shift),
        5 => v.wrapping_shr(shift),
        6 => v.cast_unsigned().wrapping_shr(shift).cast_signed(),
        7 => !v,
        _ => 0,
    };
    f64::from(r)
}

/// `binom(n, k)`: `n` and `k` as `int`s (`Binomial.run@95-127`); see the
/// [module docs](self#numeric-semantics) on why this is not legacy's.
fn binom(n: f64, k: f64) -> f64 {
    let n = java_int(n);
    let k = java_int(k);
    if n < 0 || k < 0 || k > n {
        return f64::NAN;
    }
    // C(n, k) = C(n, n - k): at most n / 2 steps. Step i makes
    // C(n - k + i, i) exactly while it is below 2^53; it is at least 2^i,
    // so it overflows to infinity, and the loop stops, within ~1100 steps.
    let k = k.min(n - k);
    let top = f64::from(n - k);
    let mut r = 1.0;
    for i in 1..=k {
        let i = f64::from(i);
        r = r * (top + i) / i;
        if r.is_infinite() {
            break;
        }
    }
    r
}

#[cfg(test)]
mod tests;
