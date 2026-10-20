//! Evaluation throughput of parsed formulas (US-RR-20-4, MF-25).
//!
//! `cargo bench -p rr-core --bench expr_eval`. A plain `harness = false`
//! benchmark, `std::hint::black_box` and the workspace's sanctioned clock
//! ([`RealClock`], D-010), so it needs no dependency. CI compiles it with
//! every `cargo clippy --all-targets`; nothing runs it, and no test depends
//! on timing (the no-allocation and concurrency proofs are
//! `tests/expr_eval_no_alloc.rs`).
//!
//! Two measurements:
//!
//! * nanoseconds per [`Expr::eval`] for a few representative formulas: a
//!   linear ROM scaling, a logger conversion built from `BitWise`, a derived
//!   parameter, and formulas at the default parser limits (the most nodes,
//!   the deepest evaluation stack);
//! * throughput of one shared set of those formulas, by reference, on 1 and
//!   on N threads. With no lock and no shared mutable state, N threads do
//!   close to N times the work of one, up to the machine's physical cores;
//!   legacy ran every evaluation in the program under one `synchronized`
//!   lock (`util/JEPUtil.java:46`, `:62`), so its N-thread throughput was
//!   at most its 1-thread throughput.
//!
//! Measured on the development machine (Intel Core i5-12600: 6 cores, 12
//! hardware threads; release profile, US-RR-20-4), two runs agreeing to
//! within 2%:
//!
//! | Formula | Nodes | ns/eval |
//! |---------|-------|---------|
//! | `x*0.01` | 3 | 9.5 |
//! | `BitWise(24,x,1)+BitWise(1,x,2)+BitWise(3,x,3)+BitWise(2,x,4)` | 19 | 42 |
//! | `([P9:km/h]/3600)/((P12/[P58:AFR])/761)` | 9 | 19 |
//! | `x+x+...+x`, 512 terms (the node limit) | 1023 | 960 |
//! | `x+(x+(...(x)...))`, stack depth 32 (the stack limit) | 63 | 90 |
//!
//! | Threads sharing the set | Evaluations/s | vs 1 thread |
//! |-------------------------|---------------|-------------|
//! | 1 | 4.45 M | 1.0x |
//! | 2 | 8.8 M | 2.0x |
//! | 6 | 25.8 M | 5.8x |
//! | 12 | 26.5 M | 5.9x |
//!
//! Linear up to the six physical cores; the second hardware thread of a
//! core adds almost nothing to this compute-bound loop, as expected. A
//! small formula costs mostly the fixed part of a call (checking the
//! bindings, the 32-value inline stack, the finiteness check), about 1 ns a
//! node after that.

use std::hint::black_box;
use std::time::Duration;

use rr_core::clock::{Clock, RealClock};
use rr_core::expr::{self, Expr};

/// Formulas measured one at a time: (label, formula).
fn formulas() -> Vec<(&'static str, String)> {
    let open = expr::INLINE_STACK - 1;
    vec![
        ("scaling x*0.01", "x*0.01".to_owned()),
        (
            "logger BitWise",
            "BitWise(24,x,1)+BitWise(1,x,2)+BitWise(3,x,3)+BitWise(2,x,4)".to_owned(),
        ),
        (
            "derived parameter",
            "([P9:km/h]/3600)/((P12/[P58:AFR])/761)".to_owned(),
        ),
        ("1023 nodes (node limit)", vec!["x"; 512].join("+")),
        (
            "stack depth 32 (stack limit)",
            format!("{}x{}", "x+(".repeat(open), ")".repeat(open)),
        ),
    ]
}

/// Evaluates `e` `n` times over a sweep of `x`, returning the sum so the
/// work cannot be optimised away.
fn run(e: &Expr, params: &[f64], n: u32) -> f64 {
    let mut acc = 0.0;
    for i in 0..n {
        let x = black_box(f64::from(i & 0xffff));
        acc += e.eval(x, black_box(params)).unwrap_or(0.0);
    }
    acc
}

/// Nanoseconds per evaluation of `e`, the best of five timed runs of a
/// count sized to take about 50 ms.
fn ns_per_eval(clock: &RealClock, e: &Expr, params: &[f64]) -> f64 {
    let mut n: u32 = 1000;
    loop {
        let start = clock.now();
        black_box(run(e, params, n));
        let took = clock.now().saturating_duration_since(start);
        if took >= Duration::from_millis(50) || n >= 1 << 30 {
            break;
        }
        n = n.saturating_mul(2);
    }
    (0..5)
        .map(|_| {
            let start = clock.now();
            black_box(run(e, params, n));
            clock.now().saturating_duration_since(start).as_secs_f64() * 1e9 / f64::from(n)
        })
        .fold(f64::INFINITY, f64::min)
}

/// Evaluations per second of the whole shared set, `rounds` sweeps of it
/// on each of `threads` threads.
fn throughput(clock: &RealClock, set: &[(Expr, Vec<f64>)], threads: u32, rounds: u32) -> f64 {
    let start = clock.now();
    std::thread::scope(|s| {
        for _ in 0..threads {
            s.spawn(|| {
                for (e, params) in set {
                    black_box(run(e, params, rounds));
                }
            });
        }
    });
    let secs = clock.now().saturating_duration_since(start).as_secs_f64();
    let evals = f64::from(threads) * f64::from(rounds) * set.len() as f64;
    evals / secs
}

fn main() -> Result<(), expr::ParseError> {
    let clock = RealClock::new();
    let mut set = Vec::new();
    println!("expr eval, ns per Expr::eval (best of 5):");
    for (label, text) in formulas() {
        let e = expr::parse(&text)?;
        let params = vec![3.0; e.params().len()];
        let ns = ns_per_eval(&clock, &e, &params);
        println!("  {label:<30} {ns:>9.1} ns  ({} nodes)", e.node_count());
        set.push((e, params));
    }

    // The node-limit formula dominates a sweep; 20_000 rounds of the set is
    // about 25 ms of work per thread here.
    let rounds = 20_000;
    let cpus = std::thread::available_parallelism().map_or(1, |n| n.get());
    let n = u32::try_from(cpus).unwrap_or(1).max(2);
    let best = |threads| {
        (0..3)
            .map(|_| throughput(&clock, &set, threads, rounds))
            .fold(0.0, f64::max)
    };
    let one = best(1);
    println!("shared set, one &Expr per formula, best of 3:");
    println!("  1 thread:  {:>12.0} evals/s  1.0x", one);
    for threads in [2, n / 2, n] {
        if threads > 1 {
            let t = best(threads);
            println!("  {threads} threads: {t:>12.0} evals/s  {:.1}x", t / one);
        }
    }
    Ok(())
}
