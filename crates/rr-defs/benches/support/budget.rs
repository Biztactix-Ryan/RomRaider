//! The budget gate of the `resolve_lookup` bench (US-RR-34-4; MF-18, MF-40,
//! EC-10): the budgets, the tolerance, and the pure decision `--check`
//! makes from the measurements.
//!
//! Shared by the bench (`benches/resolve_lookup.rs`, which measures and
//! prints) and by `tests/perf_budget_gate.rs` (which proves, with made-up
//! durations and no clock, that the gate fails above budget × [`TOLERANCE`]
//! and passes within it), both through `#[path]`: the decision the tests
//! check is the one the `perf-budget` CI job runs.

use std::time::Duration;

/// Resolving a 5000-table definition with a fresh resolver (MF-18, EC-10).
pub const RESOLVE_BUDGET: Duration = Duration::from_millis(200);

/// Finding an image's definition in the ROM-ID index (MF-40).
pub const LOOKUP_BUDGET: Duration = Duration::from_millis(5);

/// `--check` fails a measurement only above its budget times this; see the
/// bench's module docs.
pub const TOLERANCE: f64 = 1.5;

/// How a measurement compares with its budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// At or under the budget.
    Within,
    /// Over the budget, at or under budget × [`TOLERANCE`]: passes with a
    /// warning.
    OverBudget,
    /// Over budget × [`TOLERANCE`]: fails the gate.
    OverTolerance,
}

/// The longest `took` that passes `budget`: budget × [`TOLERANCE`].
pub fn limit(budget: Duration) -> Duration {
    budget.mul_f64(TOLERANCE)
}

/// How `took` compares with `budget`.
pub fn verdict(took: Duration, budget: Duration) -> Verdict {
    if took > limit(budget) {
        Verdict::OverTolerance
    } else if took > budget {
        Verdict::OverBudget
    } else {
        Verdict::Within
    }
}

/// One budgeted measurement: what, how long it took, its budget.
#[derive(Debug, Clone, Copy)]
pub struct Measurement {
    pub what: &'static str,
    pub took: Duration,
    pub budget: Duration,
}

/// The gate's decision on a run's measurements.
#[derive(Debug)]
pub struct Gate {
    /// No measurement is over budget × [`TOLERANCE`]: `--check` exits 0.
    pub passed: bool,
    /// One line per measurement, to print: `  ok  ...`, or a GitHub
    /// `::warning` (over budget) or `::error` (over the tolerance)
    /// annotation.
    pub lines: Vec<String>,
}

/// The gate's decision on `measured`: it fails if any measurement is over
/// its budget × [`TOLERANCE`].
pub fn gate(measured: &[Measurement]) -> Gate {
    let mut passed = true;
    let lines = measured
        .iter()
        .map(|m| {
            let line = format!(
                "{}: {} (budget {}, fails above {} = budget x {TOLERANCE})",
                m.what,
                ms(m.took),
                ms(m.budget),
                ms(limit(m.budget))
            );
            match verdict(m.took, m.budget) {
                Verdict::Within => format!("  ok  {line}"),
                Verdict::OverBudget => {
                    format!("::warning title=perf budget::{line}: over budget, within tolerance")
                }
                Verdict::OverTolerance => {
                    passed = false;
                    format!("::error title=perf budget::{line}")
                }
            }
        })
        .collect();
    Gate { passed, lines }
}

/// `d` in milliseconds, or microseconds below one.
pub fn ms(d: Duration) -> String {
    if d < Duration::from_millis(1) {
        format!("{:.2} µs", d.as_secs_f64() * 1e6)
    } else {
        format!("{:.1} ms", d.as_secs_f64() * 1e3)
    }
}
