//! Body of the `expr` fuzz target (task US-RR-20-9): formulas from
//! untrusted definition packs (SECURITY.md *Formulas*) through
//! `rr_core::expr`'s parser and evaluator.
//!
//! # Input layout
//!
//! `<formula> [ NUL <mode> <x: 8 bytes> <param 0: 8 bytes> ... ]`
//!
//! * **Formula**: the bytes before the first NUL (a NUL is never part of a
//!   formula), as UTF-8. Invalid UTF-8 is cut at the first bad byte
//!   (`Utf8Error::valid_up_to`) rather than replaced: no copy, and every
//!   error span stays a byte range of what the parser saw. A seed is plain
//!   formula text, so the KAT formulas are seeds as they are.
//! * **Tail** (optional, after the NUL): `mode` scales the tightened limits
//!   (below); then little-endian `f64`s, the first for `x` and the rest for
//!   the parameter references in order. Without a tail, or when it runs out,
//!   values come from [`PALETTE`] (zeros of both signs, integer edges,
//!   extremes, NaN and the infinities), and with no `x` in the tail the
//!   formula is evaluated at every palette value.
//!
//! # What must hold
//!
//! Parsing within [`Limits::DEFAULT`] (what `expr::parse` uses):
//!
//! * an error's span is a range of the formula on character boundaries,
//!   its diagnostic carries its code and message, and a limit error names
//!   the limit in force and has that limit's code, `EXP0003` to `EXP0008`
//!   (`TooLong` exactly when the formula is over `max_len`, spanning all of
//!   it);
//! * a parsed formula is within every limit, its node spans lie in the
//!   formula, and it prints (`Display`) as a formula that parses back to an
//!   expression that prints the same, has the same references and evaluates
//!   bit for bit the same;
//! * limits are monotone: what parses within a tighter set parses the same
//!   within the default set, what parses within the defaults parses the same
//!   within a raised set, and a non-limit error under the defaults is the
//!   same error under the raised set.
//!
//! Evaluating (default and raised parses, so `eval`'s heap-stack path for
//! formulas deeper than `INLINE_STACK` is covered too):
//!
//! * `eval`, `eval_with` on a fresh stack and `eval_with` on a stack reused
//!   across formulas and values agree bit for bit, or fail with the same
//!   error; `Ok` is finite and `NonFinite` carries a non-finite value;
//! * one parameter value too many or too few is `EvalError::Bindings`.
//!
//! The harness allocates only in proportion to the input: one parameter
//! buffer the size of `Expr::params`, the printed formula, and the
//! diagnostic's message.

use rr_core::diag::{Provenance, Span, codes};
use rr_core::expr::{self, EvalError, EvalStack, Expr, Limits, MAX_DEPTH, ParseErrorKind};

/// Values `x` and the parameters take when the input's tail does not supply
/// them.
pub const PALETTE: [f64; 16] = [
    0.0,
    -0.0,
    1.0,
    -1.0,
    0.5,
    255.0,
    32768.0,
    65535.0,
    -2_147_483_648.0,
    4_294_967_296.0,
    1e300,
    f64::MIN_POSITIVE,
    f64::MAX,
    f64::NAN,
    f64::INFINITY,
    f64::NEG_INFINITY,
];

/// Mode used when the input has no tail.
const DEFAULT_MODE: u8 = 3;

/// Limits no larger than the defaults in any field, scaled by `mode`.
fn tight(mode: u8) -> Limits {
    let d = Limits::DEFAULT;
    let s = u32::from(mode) + 1; // 1..=256
    let mut l = Limits::DEFAULT;
    l.max_len = d.max_len.min(s * 16);
    l.max_literal_len = d.max_literal_len.min(s);
    l.max_nodes = d.max_nodes.min(s * 4);
    l.max_depth = d.max_depth.min(s);
    l.max_args = d.max_args.min(s);
    l.max_stack = d.max_stack.min(s);
    l
}

/// Limits no smaller than the defaults in any field: long enough to read
/// back any printed default-limit formula, deep enough (up to the
/// [`MAX_DEPTH`] ceiling) to need a heap evaluation stack.
fn raised() -> Limits {
    let mut l = Limits::DEFAULT;
    l.max_len = 1 << 20;
    l.max_literal_len = 1024;
    l.max_nodes = 4096;
    l.max_depth = MAX_DEPTH;
    l.max_args = 256;
    l.max_stack = 1024;
    l
}

/// Fuzz target: see the module docs.
pub fn expr(data: &[u8]) {
    let (formula, tail) = match data.iter().position(|&b| b == 0) {
        Some(nul) => (&data[..nul], Some(&data[nul + 1..])),
        None => (data, None),
    };
    let src = match std::str::from_utf8(formula) {
        Ok(s) => s,
        // `valid_up_to` is a character boundary, so this cannot fail.
        Err(e) => std::str::from_utf8(&formula[..e.valid_up_to()]).unwrap_or(""),
    };
    let tail = tail.unwrap_or(&[]);
    let (mode, values) = match tail.split_first() {
        Some((&m, rest)) => (m, rest),
        None => (DEFAULT_MODE, tail),
    };
    let values = Values(values);

    let default = check_parse(src, Limits::DEFAULT);
    let raised_parse = check_parse(src, raised());
    let tight_parse = check_parse(src, tight(mode));

    if let Ok(t) = &tight_parse {
        assert_eq!(
            default.as_ref().ok(),
            Some(t),
            "parses within tighter limits but not the same within the defaults: {src:?}"
        );
    }
    match &default {
        Ok(e) => {
            assert_eq!(
                raised_parse.as_ref().ok(),
                Some(e),
                "parses within the defaults but not the same within raised limits: {src:?}"
            );
        }
        Err(err) if !is_limit(err.kind()) => {
            assert_eq!(
                raised_parse.as_ref().err(),
                Some(err),
                "a non-limit error changed when the limits were raised: {src:?}"
            );
        }
        Err(_) => {}
    }

    let mut shared = EvalStack::new();
    if let Ok(e) = &default {
        round_trip(e, &values, &mut shared);
        check_eval(e, &values, &mut shared);
    }
    if let Ok(e) = &raised_parse {
        check_eval(e, &values, &mut shared);
    }
}

/// Parses `src` within `limits` and checks what must hold of the result.
fn check_parse(src: &str, limits: Limits) -> Result<Expr, expr::ParseError> {
    let len = src.len();
    let over_len = u32::try_from(len).map_or(true, |l| l > limits.max_len);
    let result = expr::parse_with(src, limits);
    match &result {
        Ok(e) => {
            assert!(!over_len, "{len}-byte formula accepted over max_len");
            assert!(e.node_count() >= 1);
            assert!(e.node_count() <= limits.max_nodes as usize);
            assert!(e.stack_depth() <= limits.max_stack as usize);
            assert!(e.stack_depth() >= 1);
            assert!(e.params().len() <= e.node_count());
            for node in e.nodes() {
                let r = node.span().range();
                assert!(
                    src.get(r.clone()).is_some(),
                    "node span {r:?} outside {src:?}"
                );
            }
        }
        Err(err) => {
            let r = err.span().range();
            assert!(
                src.get(r.clone()).is_some(),
                "error span {r:?} not a range of the {len}-byte formula: {err:?}"
            );
            let diag = err.to_diagnostic(Provenance::new(
                "fuzz",
                Span::byte_offset(u64::from(err.span().start())),
            ));
            assert!(diag.is_error());
            assert_eq!(diag.code(), err.code());
            assert_eq!(diag.code(), err.kind().code());
            assert!(!diag.message().is_empty());
            assert_eq!(diag.message(), err.to_string());
            check_limit_error(err.kind(), err.span().range(), len, limits);
            if over_len {
                assert!(
                    matches!(err.kind(), ParseErrorKind::TooLong { .. }),
                    "{len}-byte formula over max_len {} refused for {err:?}",
                    limits.max_len
                );
            }
        }
    }
    result
}

/// A limit error names the limit in force and has that limit's own
/// diagnostic code (`EXP0003` to `EXP0008`, docs/diagnostics.md); `TooLong`
/// spans the formula.
fn check_limit_error(kind: &ParseErrorKind, span: std::ops::Range<usize>, len: usize, l: Limits) {
    let want = match *kind {
        ParseErrorKind::TooLong { len: n, limit } => {
            assert_eq!((n, limit), (len, l.max_len));
            assert_eq!(span, 0..len);
            codes::EXPR_TOO_LONG
        }
        ParseErrorKind::LiteralTooLong { len: n, limit, .. } => {
            assert_eq!(limit, l.max_literal_len);
            assert!(n > limit as usize);
            codes::EXPR_LITERAL_TOO_LONG
        }
        ParseErrorKind::TooManyNodes { limit } => {
            assert_eq!(limit, l.max_nodes);
            codes::EXPR_TOO_MANY_NODES
        }
        ParseErrorKind::TooDeep { limit } => {
            assert_eq!(limit, l.effective_depth());
            codes::EXPR_TOO_COMPLEX
        }
        ParseErrorKind::TooManyArgs { limit, .. } => {
            assert_eq!(limit, l.max_args);
            codes::EXPR_TOO_MANY_ARGS
        }
        ParseErrorKind::StackTooDeep { limit } => {
            assert_eq!(limit, l.max_stack);
            codes::EXPR_STACK_TOO_DEEP
        }
        _ => return,
    };
    assert_eq!(kind.code(), want, "{kind:?}");
}

fn is_limit(kind: &ParseErrorKind) -> bool {
    matches!(
        kind,
        ParseErrorKind::TooLong { .. }
            | ParseErrorKind::LiteralTooLong { .. }
            | ParseErrorKind::TooManyNodes { .. }
            | ParseErrorKind::TooDeep { .. }
            | ParseErrorKind::TooManyArgs { .. }
            | ParseErrorKind::StackTooDeep { .. }
    )
}

/// `Display` reads back to an expression that prints the same, has the same
/// references and evaluates the same.
fn round_trip(e: &Expr, values: &Values<'_>, stack: &mut EvalStack) {
    let printed = e.to_string();
    let back = match expr::parse_with(&printed, raised()) {
        Ok(b) => b,
        Err(err) => panic!("printed form {printed:?} does not parse: {err:?}"),
    };
    assert_eq!(back.to_string(), printed, "printing is not idempotent");
    assert_eq!(back.params(), e.params());
    assert_eq!(back.uses_x(), e.uses_x());
    assert_eq!(back.node_count(), e.node_count(), "{printed:?}");
    assert_eq!(back.stack_depth(), e.stack_depth(), "{printed:?}");
    let params = values.params(e.params().len(), 0);
    let x = values.x().unwrap_or(PALETTE[2]);
    same(
        e.eval_with(stack, x, &params),
        back.eval_with(stack, x, &params),
        "original vs printed form",
    );
}

/// Every evaluation path agrees; results obey the error contract.
fn check_eval(e: &Expr, values: &Values<'_>, shared: &mut EvalStack) {
    let n = e.params().len();
    let one = |x: f64, k: usize, shared: &mut EvalStack| {
        let params = values.params(n, k);
        let a = e.eval(x, &params);
        let b = e.eval_with(&mut EvalStack::new(), x, &params);
        let c = e.eval_with(shared, x, &params);
        assert!(shared.capacity() >= e.stack_depth());
        same(a, b, "eval vs eval_with (fresh stack)");
        same(a, c, "eval vs eval_with (reused stack)");
        same(a, e.eval(x, &params), "eval twice");
        match a {
            Ok(v) => assert!(v.is_finite(), "Ok({v})"),
            Err(EvalError::NonFinite(v)) => assert!(!v.is_finite(), "NonFinite({v})"),
            Err(other) => panic!("unexpected {other:?} with {n} parameter value(s)"),
        }
    };
    match values.x() {
        Some(x) => one(x, 0, shared),
        None => {
            for (k, &x) in PALETTE.iter().enumerate() {
                one(x, k, shared);
            }
        }
    }

    let x = values.x().unwrap_or(PALETTE[0]);
    let more = values.params(n + 1, 0);
    let want = |found| Err(EvalError::Bindings { expected: n, found });
    assert_eq!(e.eval(x, &more), want(n + 1));
    assert_eq!(e.eval_with(shared, x, &more), want(n + 1));
    if n > 0 {
        assert_eq!(e.eval(x, &more[..n - 1]), want(n - 1));
        assert_eq!(e.eval_with(shared, x, &more[..n - 1]), want(n - 1));
    }
}

/// Two results are the same: equal bits, or the same error (a `NonFinite`
/// value compared by bits too, so NaN equals the same NaN).
fn same(a: Result<f64, EvalError>, b: Result<f64, EvalError>, what: &str) {
    let ok = match (a, b) {
        (Ok(x), Ok(y)) | (Err(EvalError::NonFinite(x)), Err(EvalError::NonFinite(y))) => {
            x.to_bits() == y.to_bits()
        }
        (Err(x), Err(y)) => x == y,
        _ => false,
    };
    assert!(ok, "{what}: {a:?} != {b:?}");
}

/// The input's tail after the mode byte: little-endian `f64`s.
struct Values<'a>(&'a [u8]);

impl Values<'_> {
    fn get(&self, i: usize) -> Option<f64> {
        let at = i.checked_mul(8)?;
        let bytes = self.0.get(at..at.checked_add(8)?)?;
        Some(f64::from_le_bytes(bytes.try_into().ok()?))
    }

    /// `x`, if the tail has one.
    fn x(&self) -> Option<f64> {
        self.get(0)
    }

    /// `n` parameter values: from the tail after `x`, then the palette
    /// rotated by `k`.
    fn params(&self, n: usize, k: usize) -> Vec<f64> {
        (0..n)
            .map(|i| self.get(i + 1).unwrap_or(PALETTE[(i + k) % PALETTE.len()]))
            .collect()
    }
}
