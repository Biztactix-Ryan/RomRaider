//! The `resolve_lookup` bench's `--check` gate fails a blown budget
//! (US-RR-34-4; MF-18, MF-40, EC-10).
//!
//! The gate's decision is `benches/support/budget.rs`, included here by
//! the same `#[path]` the bench uses, so what these tests prove is what the
//! `perf-budget` CI job runs: `gate(..).passed` is the bench's exit status
//! (`false` exits 1 and fails the job, and with it the required `ci-ok`
//! check). Every duration here is made up: nothing reads a clock, so no
//! test depends on how fast the machine is.
//!
//! It checks that a measurement over budget × tolerance fails the gate with
//! an `::error` annotation naming it, one just within passes (with a
//! `::warning` when over the budget itself), the boundary is inclusive, one
//! failing measurement fails the whole run, and the budgets and tolerance
//! are the story's: resolve 200 ms, lookup 5 ms, × 1.5.
//! `xtask/tests/perf_budget_ci_config.rs` pins the CI wiring.

#[path = "../benches/support/budget.rs"]
mod budget;

use std::time::Duration;

use budget::{
    LOOKUP_BUDGET, Measurement, RESOLVE_BUDGET, TOLERANCE, Verdict, gate, limit, ms, verdict,
};

const MS: Duration = Duration::from_millis(1);
const NS: Duration = Duration::from_nanos(1);

fn resolve(took: Duration) -> Measurement {
    Measurement {
        what: "resolve",
        took,
        budget: RESOLVE_BUDGET,
    }
}

fn lookup(took: Duration) -> Measurement {
    Measurement {
        what: "lookup",
        took,
        budget: LOOKUP_BUDGET,
    }
}

#[test]
fn budgets_are_the_storys() {
    assert_eq!(RESOLVE_BUDGET, 200 * MS);
    assert_eq!(LOOKUP_BUDGET, 5 * MS);
    assert!((TOLERANCE - 1.5).abs() < f64::EPSILON);
    assert_eq!(limit(RESOLVE_BUDGET), 300 * MS);
    assert_eq!(limit(LOOKUP_BUDGET), Duration::from_micros(7500));
}

#[test]
fn over_tolerance_fails_the_gate() {
    for m in [
        resolve(limit(RESOLVE_BUDGET) + NS),
        resolve(457 * MS),
        resolve(Duration::MAX),
        lookup(limit(LOOKUP_BUDGET) + NS),
        lookup(Duration::from_secs(1)),
    ] {
        assert_eq!(verdict(m.took, m.budget), Verdict::OverTolerance, "{m:?}");
        let g = gate(&[m]);
        assert!(!g.passed, "{m:?} passed: {:?}", g.lines);
        assert_eq!(g.lines.len(), 1);
        assert!(
            g.lines[0].starts_with(&format!("::error title=perf budget::{}: ", m.what)),
            "{:?}",
            g.lines
        );
    }
}

#[test]
fn within_tolerance_passes_the_gate() {
    for (m, want) in [
        (resolve(Duration::ZERO), Verdict::Within),
        (resolve(127 * MS), Verdict::Within),
        (resolve(RESOLVE_BUDGET), Verdict::Within),
        (resolve(RESOLVE_BUDGET + NS), Verdict::OverBudget),
        (resolve(limit(RESOLVE_BUDGET)), Verdict::OverBudget),
        (lookup(Duration::from_nanos(420)), Verdict::Within),
        (lookup(LOOKUP_BUDGET), Verdict::Within),
        (lookup(limit(LOOKUP_BUDGET)), Verdict::OverBudget),
    ] {
        assert_eq!(verdict(m.took, m.budget), want, "{m:?}");
        let g = gate(&[m]);
        assert!(g.passed, "{m:?} failed: {:?}", g.lines);
        let prefix = match want {
            Verdict::Within => format!("  ok  {}: ", m.what),
            _ => format!("::warning title=perf budget::{}: ", m.what),
        };
        assert!(g.lines[0].starts_with(&prefix), "{:?}", g.lines);
    }
}

#[test]
fn one_blown_budget_fails_the_run() {
    let g = gate(&[resolve(127 * MS), lookup(8 * MS)]);
    assert!(!g.passed);
    assert!(g.lines[0].starts_with("  ok  resolve: "), "{:?}", g.lines);
    assert_eq!(
        g.lines[1],
        "::error title=perf budget::lookup: 8.0 ms (budget 5.0 ms, fails above 7.5 ms = budget x 1.5)"
    );
    assert!(gate(&[resolve(127 * MS), lookup(Duration::from_nanos(420))]).passed);
}

#[test]
fn a_zero_budget_fails_any_measurement() {
    let g = gate(&[Measurement {
        what: "resolve",
        took: NS,
        budget: Duration::ZERO,
    }]);
    assert!(!g.passed, "{:?}", g.lines);
}

#[test]
fn durations_print_in_ms_or_us() {
    assert_eq!(ms(Duration::from_nanos(420)), "0.42 µs");
    assert_eq!(ms(Duration::from_micros(127_200)), "127.2 ms");
}
