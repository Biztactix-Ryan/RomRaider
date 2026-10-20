//! Evaluating a parsed formula allocates nothing (US-RR-20-7, MF-25), and
//! refusing a formula over a parser limit allocates boundedly (US-RR-20-8).
//!
//! US-RR-20-4 (MF-25): evaluation of a parsed AST performs no allocation
//! and no locking. Legacy evaluated every formula in the program behind one
//! `synchronized` method and one synchronized 32-entry cache keyed by the
//! formula text (`util/JEPUtil.java:44`, `:46`, `:62`), and a derived logger
//! parameter built a `HashMap` and ran `String.replace` on its formula for
//! every sample (`logger/ecu/definition/EcuDerivedParameterConvertorImpl.java:53-68`).
//! Here:
//!
//! * no allocation: every formula of the JEP known-answer fixture, every
//!   function and operator, and formulas at each parser limit, through both
//!   `eval` and `eval_with` (`evaluating_a_parsed_formula_does_not_allocate`);
//! * no locking: one set of parsed formulas evaluated from many threads at
//!   once through `&Expr` agrees with the single-threaded answers and
//!   allocates nothing meanwhile
//!   (`mf25_one_parsed_formula_set_is_shared_by_many_threads`); that
//!   nothing in the module could lock is a source check, `cargo xtask
//!   check-globals`, which refuses any lock, cell, atomic or lazy/once type
//!   anywhere under `rr-core/src/expr` (`STATELESS_PATHS`), since a
//!   `Mutex` field would leave `Expr` `Send + Sync`;
//! * parsed once: an `Expr` owns no formula text, and parameter values are
//!   bound by position, not name
//!   (`mf25_a_derived_parameter_is_parsed_once_and_bound_by_position`).
//!
//! The throughput side (and that it scales with threads) is the benchmark
//! `benches/expr_eval.rs`; no test here depends on timing.
//!
//! A dedicated test binary, because it replaces the global allocator with
//! one that counts every allocation, reallocation and zeroed allocation. The
//! formulas are parsed and the results buffer sized before counting starts;
//! inside the counted window there is nothing but evaluation, and every
//! check runs after it.
//!
//! The counters are per thread (`thread_local!`): an allocation is counted
//! for the thread that made it, and a window reads only its own thread's
//! counters. Other threads of the process (the test harness's main thread,
//! output capture, a timer) may allocate at any moment, and a process-wide
//! counter would charge that to the evaluation, so the result would depend
//! on scheduling. The multi-threaded test has each worker count its own
//! window and sums them. The counters are the only per-thread state this
//! binary needs: a `#[global_allocator]` has no other way to report back.
//! They are test-only and listed in `xtask/globals-allowlist.toml`.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::Barrier;

use rr_core::expr::{
    self, BinaryOp, EvalError, EvalStack, Expr, Func, INLINE_STACK, Limits, NodeKind,
    ParseErrorKind, UnaryOp,
};

/// Forwards to [`System`], counting calls that obtain memory against the
/// calling thread.
struct Counting;

thread_local! {
    /// Calls that obtained memory on this thread.
    static ALLOCATIONS: Cell<usize> = const { Cell::new(0) };
    /// Bytes obtained by those calls (for `realloc`, the new size).
    static BYTES: Cell<usize> = const { Cell::new(0) };
}

/// Counts one call obtaining `bytes` against the calling thread. Must not
/// allocate (it runs inside the allocator): a `const`-initialised `Cell`
/// of a type without `Drop` is a plain thread-local slot, with no lazy
/// initialisation and no destructor to register. `try_with` (not `with`)
/// so that a call during thread teardown is skipped rather than a panic
/// inside the allocator.
fn count(bytes: usize) {
    let _ = ALLOCATIONS.try_with(|c| c.set(c.get().wrapping_add(1)));
    let _ = BYTES.try_with(|c| c.set(c.get().wrapping_add(bytes)));
}

#[global_allocator]
static GLOBAL: Counting = Counting;

// SAFETY: every method forwards to `System` with the caller's arguments
// unchanged; the counter does not affect what is returned.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        count(layout.size());
        // SAFETY: forwarded unchanged; the caller upholds `alloc`'s contract.
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        count(layout.size());
        // SAFETY: forwarded unchanged; the caller upholds the contract.
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        count(new_size);
        // SAFETY: forwarded unchanged; `ptr` came from this allocator.
        unsafe { System.realloc(ptr, layout, new_size) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: forwarded unchanged; `ptr` came from this allocator.
        unsafe { System.dealloc(ptr, layout) };
    }
}

/// How many allocations `f` makes on the calling thread (`f` here starts
/// no threads; other threads of the process are not counted).
fn allocations_in(f: impl FnOnce()) -> usize {
    let before = ALLOCATIONS.with(Cell::get);
    f();
    ALLOCATIONS.with(Cell::get).wrapping_sub(before)
}

/// How many bytes `f` obtains on the calling thread, over all its
/// allocations (memory it frees is not subtracted, so this bounds its peak
/// too).
fn bytes_in(f: impl FnOnce()) -> usize {
    let before = BYTES.with(Cell::get);
    f();
    BYTES.with(Cell::get).wrapping_sub(before)
}

/// Real definition formulas (the pinned corpus, D-028) and every function
/// and operator.
const FORMULAS: &[&str] = &[
    "x",
    "(x-32)/1.8",
    "14.7/(1+x*.0078125)",
    "(x-7.350001)/-.000224304213",
    "if(65536+x/65536>32767,(65536+x/65536)/32768,x/65536/32768)",
    "if(x<2147483647,((x+2147483648)%65536)/8192,(x%65536)/8192)",
    "if(x%256==255,0,((x%256)-128)*100/128)",
    "([P9:km/h]/3600)/((P12/[P58:AFR])/761)",
    "[P7:psi]-[P24:psi]",
    "P156*(835-(0.7*([P31:C]-15)))/1000",
    "(P200*2*8.314472*([P11:C]+273.15))/([P7:kPa]*2.5*28.97)*100",
    "BitWise(24,x,1)+BitWise(1,x,2)+BitWise(3,x,3)+BitWise(2,x,4)",
    "BitWise(2,x,5)+BitWise(2,x,6)+BitWise(0,x,7)+BitWise(1,x,P1)",
    "(x&255)|(x<<2)|(x>>1)",
    "sin(x)+cos(x)+tan(x)+asin(x/1e6)+acos(x/1e6)+atan(x)+atan2(x,2)",
    "sinh(x/1e3)+cosh(x/1e3)+tanh(x)+asinh(x)+acosh(x+1)+atanh(x/1e9)",
    "log(x+1)+ln(x+1)+exp(x/1e3)+pow(x,0.5)+sqrt(x)+abs(-x)+mod(x,7)",
    "sum(x,1,2,3)+if(x,1,2,3)+binom(10,3)+x^2+-x+!x",
    "(x<1)+(x>1)+(x<=1)+(x>=1)+(x==1)+(x!=1)+(x&&1)+(x||0)",
    "1/(x-x)",
];

/// The JEP known-answer fixture (US-RR-20-2).
const KAT: &str = include_str!("../../../fixtures/kat/expr-jep-precedence.toml");

/// The `formula` of every `[[expr]]` case in [`KAT`], read from its
/// `formula = "..."` line (rr-core takes no TOML dependency; rr-testkit's
/// `tests/expr_kat.rs` reads the file properly and checks the values).
fn kat_formulas() -> Vec<&'static str> {
    let formulas: Vec<&str> = KAT
        .lines()
        .filter_map(|l| l.strip_prefix("formula = \"")?.strip_suffix('"'))
        .collect();
    // As many as `[[expr]]` cases, none needing a TOML escape.
    assert_eq!(formulas.len(), KAT.matches("\n[[expr]]\n").count());
    assert!(formulas.len() >= 100, "{} KAT formulas", formulas.len());
    assert!(formulas.iter().all(|f| !f.contains(['\\', '"'])));
    formulas
}

/// The largest `n` up to `upper` for which `make(n)` parses within the
/// default limits, checked to be a limit: `make(n + 1)` is refused.
#[expect(
    clippy::unwrap_used,
    reason = "test-only helper outside a #[test] fn; panicking is how it fails the calling test"
)]
fn at_the_limit(upper: usize, make: impl Fn(usize) -> String) -> String {
    let n = (1..=upper)
        .rev()
        .find(|&n| expr::parse(&make(n)).is_ok())
        .unwrap();
    assert!(n < upper, "no limit below {upper}");
    assert!(expr::parse(&make(n + 1)).is_err(), "{n} is not the limit");
    make(n)
}

/// A formula at each default parser limit (US-RR-20-8).
fn limit_formulas() -> Vec<String> {
    let open = INLINE_STACK - 1;
    vec![
        // The deepest evaluation stack: a right-nested chain, and a call.
        format!("{}x{}", "x+(".repeat(open), ")".repeat(open)),
        sum_text(INLINE_STACK),
        // The deepest nesting.
        at_the_limit(1000, |n| format!("{}x{}", "(".repeat(n), ")".repeat(n))),
        // The most nodes.
        at_the_limit(2000, |n| vec!["x"; n].join("+")),
        // The longest formula and the longest literal.
        at_the_limit(5000, |n| format!("x{}", " ".repeat(n))),
        at_the_limit(200, |n| format!("x*1{}", "0".repeat(n))),
    ]
}

/// Everything the no-allocation tests evaluate: [`FORMULAS`], the KAT
/// formulas and [`limit_formulas`], parsed, checked to call every function
/// and use every operator.
#[expect(
    clippy::unwrap_used,
    reason = "test-only helper outside a #[test] fn; panicking is how it fails the calling test"
)]
fn representative() -> Vec<Expr> {
    let mut texts: Vec<String> = FORMULAS.iter().map(|&s| s.to_owned()).collect();
    texts.extend(kat_formulas().into_iter().map(str::to_owned));
    texts.extend(limit_formulas());
    let exprs: Vec<Expr> = texts.iter().map(|s| expr::parse(s).unwrap()).collect();

    let (mut funcs, mut binary, mut unary) = (Vec::new(), Vec::new(), Vec::new());
    for node in exprs.iter().flat_map(Expr::nodes) {
        match *node.kind() {
            NodeKind::Call { func, .. } => funcs.push(func),
            NodeKind::Binary { op, .. } => binary.push(op),
            NodeKind::Unary { op, .. } => unary.push(op),
            _ => {}
        }
    }
    for f in Func::ALL {
        assert!(funcs.contains(&f), "no formula calls {}", f.name());
    }
    for op in BinaryOp::ALL {
        assert!(binary.contains(&op), "no formula uses {}", op.symbol());
    }
    for op in [UnaryOp::Neg, UnaryOp::Not] {
        assert!(unary.contains(&op), "no formula uses {op:?}");
    }
    exprs
}

/// One value per parameter reference of each formula.
fn bindings_for(exprs: &[Expr]) -> Vec<Vec<f64>> {
    exprs.iter().map(|e| vec![3.0; e.params().len()]).collect()
}

/// Whether two results are the same, bit for bit (NaN included).
fn same(a: Result<f64, EvalError>, b: Result<f64, EvalError>) -> bool {
    match (a, b) {
        (Ok(a), Ok(b)) | (Err(EvalError::NonFinite(a)), Err(EvalError::NonFinite(b))) => {
            a.to_bits() == b.to_bits()
        }
        (a, b) => a == b,
    }
}

#[test]
fn evaluating_a_parsed_formula_does_not_allocate() {
    let exprs = representative();
    let bindings = bindings_for(&exprs);
    let mut stacks: Vec<EvalStack> = exprs.iter().map(EvalStack::for_expr).collect();
    // A caller stack that grows on first use: after one round, none.
    let mut shared = EvalStack::new();
    for (e, params) in exprs.iter().zip(&bindings) {
        let _ = e.eval_with(&mut shared, 0.0, params);
    }
    let mut ok = 0usize;
    let mut err = 0usize;
    let mut differ = 0usize;

    let count = allocations_in(|| {
        for i in 0..1000u32 {
            let x = f64::from(i);
            for ((e, params), stack) in exprs.iter().zip(&bindings).zip(&mut stacks) {
                let inline = e.eval(x, params);
                let sized = e.eval_with(stack, x, params);
                let grown = e.eval_with(&mut shared, x, params);
                if !same(inline, sized) || !same(inline, grown) {
                    differ += 1;
                }
                match inline {
                    Ok(_) => ok += 1,
                    Err(_) => err += 1,
                }
            }
        }
    });

    assert_eq!(count, 0, "evaluation allocated {count} time(s)");
    assert_eq!(differ, 0, "eval and eval_with disagreed");
    // Every evaluation ran; the errors are the non-finite results (and they
    // allocate nothing either).
    assert_eq!(ok + err, 1000 * exprs.len());
    assert!(ok > 0 && err > 0);
}

/// Folds every result of evaluating `exprs` (with `eval` and `eval_with`)
/// over thread `t`'s sweep of `x` into one number, allocating nothing.
fn digest(exprs: &[Expr], bindings: &[Vec<f64>], stacks: &mut [EvalStack], t: u32) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut mix = |r: Result<f64, EvalError>| {
        let bits = match r {
            Ok(v) => v.to_bits(),
            Err(EvalError::NonFinite(v)) => !v.to_bits(),
            Err(_) => u64::MAX,
        };
        h = (h ^ bits).wrapping_mul(0x0000_0100_0000_01b3);
    };
    for i in 0..200u32 {
        let x = f64::from(i * 13 + t);
        for ((e, params), stack) in exprs.iter().zip(bindings).zip(&mut *stacks) {
            mix(e.eval(x, params));
            mix(e.eval_with(stack, x, params));
        }
    }
    h
}

/// MF-25: legacy serialised every evaluation on one lock around one global
/// cache (`util/JEPUtil.java:44`, `:46`, `:62`). Here one set of parsed
/// formulas is shared, by reference, by many threads evaluating at once:
/// each gets the single-threaded answers, and nothing allocates while they
/// run. (That nothing can lock is `cargo xtask check-globals`; see the
/// module docs.)
#[test]
fn mf25_one_parsed_formula_set_is_shared_by_many_threads() {
    const fn shareable<T: Send + Sync + 'static>() {}
    shareable::<Expr>();
    shareable::<Vec<Expr>>();

    const THREADS: u32 = 8;
    let exprs = representative();
    let bindings = bindings_for(&exprs);
    let fresh = || -> Vec<EvalStack> { exprs.iter().map(EvalStack::for_expr).collect() };
    let expected: Vec<u64> = (0..THREADS)
        .map(|t| digest(&exprs, &bindings, &mut fresh(), t))
        .collect();

    // One barrier, waited on three times by every thread and this one:
    // all ready (stacks built, the barrier's own lazy state created), go,
    // done, so the workers evaluate at the same time. Each worker counts
    // its own window, from "go" to the end of its evaluation, in which only
    // evaluation runs; the windows are summed.
    let barrier = Barrier::new(THREADS as usize + 1);
    let (exprs, bindings) = (&exprs, &bindings);
    let results: Vec<(u64, usize)> = std::thread::scope(|s| {
        let handles: Vec<_> = (0..THREADS)
            .map(|t| {
                let mut stacks = fresh();
                let barrier = &barrier;
                s.spawn(move || {
                    barrier.wait();
                    barrier.wait();
                    let mut d = 0;
                    let count = allocations_in(|| d = digest(exprs, bindings, &mut stacks, t));
                    barrier.wait();
                    (d, count)
                })
            })
            .collect();
        barrier.wait();
        barrier.wait();
        barrier.wait();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });
    let got: Vec<u64> = results.iter().map(|&(d, _)| d).collect();
    let count: usize = results.iter().map(|&(_, c)| c).sum();

    assert_eq!(got, expected);
    assert_eq!(count, 0, "concurrent evaluation allocated {count} time(s)");
}

/// MF-25, parsed once: legacy rewrote a derived parameter's formula with
/// `String.replace` and filled a `HashMap` by name on every sample, then
/// looked the text up in the cache
/// (`logger/ecu/definition/EcuDerivedParameterConvertorImpl.java:53-68`,
/// `util/JEPUtil.java:62-82`). Here the formula is parsed once into an
/// `Expr` that owns no formula text (`'static`, so it borrows none; the
/// text is dropped before evaluating), each reference is resolved to a
/// position when it is bound, and per sample the caller only fills a slice
/// by position: no names, strings or allocation per evaluation.
#[test]
fn mf25_a_derived_parameter_is_parsed_once_and_bound_by_position() {
    const fn owns_its_tree<T: 'static>() {}
    owns_its_tree::<Expr>();

    let text = String::from("([P9:km/h]/3600)/((P12/[P58:AFR])/761)");
    let e = expr::parse(&text).unwrap();
    drop(text);
    let ids: Vec<&str> = e.params().iter().map(|p| p.id()).collect();
    assert_eq!(ids, ["P9", "P12", "P58"]);
    let reference = |speed: f64, maf: f64, afr: f64| (speed / 3600.0) / ((maf / afr) / 761.0);

    let mut values = [0.0; 3];
    let mut wrong = 0usize;
    let count = allocations_in(|| {
        for i in 1..=10_000u32 {
            let i = f64::from(i);
            values = [i, i / 7.0, 14.7];
            let want = reference(i, i / 7.0, 14.7);
            if e.eval(f64::NAN, &values) != Ok(want) {
                wrong += 1;
            }
        }
    });
    assert_eq!(count, 0, "per-sample evaluation allocated {count} time(s)");
    assert_eq!(wrong, 0);

    // Binding is by position only: the same values in another order are
    // other parameters, and a slice of the wrong length is refused.
    let swapped = e.eval(f64::NAN, &[1.0, 14.7, 2.0]).unwrap();
    assert_eq!(swapped, reference(1.0, 14.7, 2.0));
    assert_ne!(swapped, reference(1.0, 2.0, 14.7));
    assert_eq!(
        e.eval(f64::NAN, &[1.0, 2.0]),
        Err(EvalError::Bindings {
            expected: 3,
            found: 2
        })
    );
}

/// `sum(1, 2, ..., n)`, a formula of stack depth `n`, as text.
fn sum_text(n: usize) -> String {
    let args: Vec<String> = (1..=n).map(|i| i.to_string()).collect();
    format!("sum({})", args.join(","))
}

/// [`sum_text`] parsed within `limits`.
fn sum_of(n: usize, limits: Limits) -> Result<Expr, expr::ParseError> {
    expr::parse_with(&sum_text(n), limits)
}

/// The default limits with the argument and stack limits lifted.
fn deep_limits() -> Limits {
    let mut limits = Limits::DEFAULT;
    limits.max_args = u32::MAX;
    limits.max_stack = u32::MAX;
    limits
}

#[test]
fn the_inline_stack_covers_formulas_up_to_its_depth() {
    let fits = sum_of(INLINE_STACK, Limits::DEFAULT).unwrap();
    assert_eq!(fits.stack_depth(), INLINE_STACK);
    let mut result = None;
    let count = allocations_in(|| result = Some(fits.eval(0.0, &[])));
    assert_eq!(count, 0);
    let n = INLINE_STACK as f64;
    assert_eq!(result, Some(Ok(n * (n + 1.0) / 2.0)));
}

#[test]
fn every_formula_within_the_default_limits_evaluates_inline() {
    // The default stack limit is the inline stack, so `eval` never needs a
    // heap buffer for a formula `parse` accepted (US-RR-20-8).
    assert_eq!(
        usize::try_from(Limits::DEFAULT.max_stack).unwrap(),
        INLINE_STACK
    );
    // A formula of the deepest stack the defaults allow, built from the
    // operator that piles up values fastest (a right-nested chain), and a
    // call at both the argument and the stack limit.
    let open = INLINE_STACK - 1;
    let chain = format!("{}x{}", "x+(".repeat(open), ")".repeat(open));
    let exprs = [
        expr::parse(&chain).unwrap(),
        sum_of(INLINE_STACK, Limits::DEFAULT).unwrap(),
    ];
    let mut results = [None, None];
    let count = allocations_in(|| {
        for (e, r) in exprs.iter().zip(&mut results) {
            *r = Some(e.eval(1.0, &[]));
        }
    });
    assert_eq!(count, 0);
    assert_eq!(exprs[0].stack_depth(), INLINE_STACK);
    assert_eq!(results[0], Some(Ok(INLINE_STACK as f64)));
    // One value more is refused at parse time instead.
    let deeper = format!("{}x{}", "x+(".repeat(open + 1), ")".repeat(open + 1));
    assert!(matches!(
        expr::parse(&deeper).unwrap_err().kind(),
        ParseErrorKind::StackTooDeep { .. }
    ));
    assert!(sum_of(INLINE_STACK + 1, Limits::DEFAULT).is_err());
}

#[test]
fn deeper_formulas_allocate_only_without_a_caller_stack() {
    // Only with raised limits.
    let deep = sum_of(INLINE_STACK + 1, deep_limits()).unwrap();
    assert_eq!(deep.stack_depth(), INLINE_STACK + 1);
    let n = (INLINE_STACK + 1) as f64;
    let expected = Some(Ok(n * (n + 1.0) / 2.0));

    // `eval` needs a heap buffer for it (documented), which shows the
    // counter sees allocations.
    let mut result = None;
    assert!(allocations_in(|| result = Some(deep.eval(0.0, &[]))) > 0);
    assert_eq!(result, expected);

    // A stack sized at bind time: none.
    let mut stack = EvalStack::for_expr(&deep);
    let mut result = None;
    let count = allocations_in(|| result = Some(deep.eval_with(&mut stack, 0.0, &[])));
    assert_eq!(count, 0);
    assert_eq!(result, expected);

    // A stack that grew on first use: none after that.
    let mut stack = EvalStack::new();
    let mut first = None;
    assert!(allocations_in(|| first = Some(deep.eval_with(&mut stack, 0.0, &[]))) > 0);
    assert_eq!(first, expected);
    let mut result = None;
    let count = allocations_in(|| {
        for _ in 0..100 {
            result = Some(deep.eval_with(&mut stack, 0.0, &[]));
        }
    });
    assert_eq!(count, 0);
    assert_eq!(result, expected);
}

/// Parses `src` within `limits`, returning the error kind (`None` if it
/// parsed) and how many allocations and bytes parsing took.
fn refuse(src: &str, limits: Limits) -> (Option<ParseErrorKind>, usize, usize) {
    let mut result = None;
    let mut count = 0;
    let bytes =
        bytes_in(|| count = allocations_in(|| result = Some(expr::parse_with(src, limits))));
    let kind = result.and_then(Result::err).map(|e| e.kind().clone());
    (kind, count, bytes)
}

#[test]
fn refusing_an_over_limit_formula_allocates_boundedly() {
    let unbounded_len = {
        let mut l = Limits::DEFAULT;
        l.max_len = u32::MAX;
        l
    };
    // Refused before anything is read or built: nothing at all.
    let huge = "(".repeat(100 << 20);
    let (kind, count, _) = refuse(&huge, Limits::DEFAULT);
    assert!(matches!(kind, Some(ParseErrorKind::TooLong { .. })));
    assert_eq!(count, 0);
    // Nesting is refused before the first node: nothing either.
    let (kind, count, _) = refuse(&huge, unbounded_len);
    assert!(matches!(kind, Some(ParseErrorKind::TooDeep { .. })));
    assert_eq!(count, 0);
    drop(huge);
    // A literal is refused before it is converted or copied.
    let literal = "9".repeat(1 << 20);
    let (kind, count, _) = refuse(&literal, unbounded_len);
    assert!(matches!(kind, Some(ParseErrorKind::LiteralTooLong { .. })));
    assert_eq!(count, 0);
    let name = format!("x*{}", "P".repeat(1 << 20));
    let (kind, _, bytes) = refuse(&name, unbounded_len);
    assert!(matches!(kind, Some(ParseErrorKind::LiteralTooLong { .. })));
    assert!(bytes < 1024, "{bytes} bytes");

    // A million-term chain stops at the node limit: the arena grew to that
    // limit and no further. 1024 nodes of at most 32 bytes, doubled for
    // `Vec` growth, is the bound.
    let chain = vec!["1"; 1_000_000].join("+");
    let (kind, _, bytes) = refuse(&chain, unbounded_len);
    assert!(matches!(kind, Some(ParseErrorKind::TooManyNodes { .. })));
    let bound = 2 * 1024 * 32;
    assert!(bytes <= bound, "{bytes} bytes for a refused chain");
    // Likewise a call with a million arguments, at the argument limit.
    let call = format!("sum({})", vec!["1"; 1_000_000].join(","));
    let (kind, _, bytes) = refuse(&call, unbounded_len);
    assert!(matches!(kind, Some(ParseErrorKind::TooManyArgs { .. })));
    assert!(bytes <= bound, "{bytes} bytes for a refused call");
    // And a million right-nested values, at the stack limit.
    let nested = format!("{}1", "1+(".repeat(1_000_000));
    let (kind, _, bytes) = refuse(&nested, unbounded_len);
    assert!(matches!(kind, Some(ParseErrorKind::StackTooDeep { .. })));
    assert!(bytes <= bound, "{bytes} bytes for a refused nest");
}
