//! CI gate wiring for story US-RR-2 (ci.yml: fmt / clippy / nextest on every
//! matrix target, cargo-deny, pending insta snapshots, the
//! i686-pc-windows-msvc test target).
//!
//! Each criterion is a pure check over the text of `.github/workflows/ci.yml`
//! that returns a list of problems. One test runs it on the committed file
//! (so the nextest jobs fail if the wiring regresses), and `*_rejects_*`
//! tests prove each assertion bites on a mutated copy of the real file.
//!
//! * **US-RR-2-1** ([`pr_gates_problems`]): a rustfmt violation, a clippy
//!   warning or a failing test blocks the PR on every matrix target. Job
//!   `fmt` runs `cargo fmt --all --check`; job `clippy` runs clippy with
//!   `-D warnings` on the host; the `test` matrix covers the four US-RR-2
//!   targets (`fail-fast: false`, no `exclude:`, `RUSTFLAGS=-D warnings`) and
//!   for each entry runs clippy (`-D warnings`) and `cargo nextest run
//!   --workspace` with `--target <entry>`, plus `cargo xtask gates-selftest
//!   --target <entry>`, which proves on that target that those exact
//!   commands fail on a planted violation (`xtask/src/gates_selftest.rs`; the
//!   commands are compared with its [`Gate::ci_command`]). No step of those
//!   jobs can be skipped or softened (`if:`, `continue-on-error`, `|| true`),
//!   none has a job-level `if:`, the default shell is bash (`-eo pipefail`,
//!   also on Windows), and `ci-ok` (the only required check) needs all three
//!   jobs, is `if: always()` and fails when any needed job's result is
//!   `failure`, `cancelled` or `skipped`.
//! * **US-RR-2-2** ([`deny_problems`]): job `deny` runs `cargo deny --locked
//!   check` (every check, no subset) and `cargo xtask deny-selftest` (which
//!   proves that command rejects a GPL-incompatible licence and an open
//!   RustSec advisory, see `xtask/src/deny_selftest.rs`), can't be skipped,
//!   and is in `ci-ok.needs`.
//! * **US-RR-2-3** ([`snapshots_problems`]): job `snapshots` fails on
//!   committed `*.snap.new` / `*.pending-snap` files (`git ls-files`, before
//!   any test runs), runs `cargo insta test --workspace --check
//!   --unreferenced=reject` with `INSTA_UPDATE=no`, finishes with an
//!   `if: always()` check that no pending file was left behind, and is in
//!   `ci-ok.needs`.
//! * **US-RR-2-4** ([`i686_problems`]): the `test` matrix has an
//!   `i686-pc-windows-msvc` entry on a Windows runner, and for that entry the
//!   job installs the target and runs `cargo nextest run --workspace` and the
//!   doctests with `--target i686-pc-windows-msvc`; `test` is in
//!   `ci-ok.needs`. Whether that run is green can only be seen on GitHub.
//! * **US-RR-18-3** ([`faketime_problems`]): job `faketime` installs
//!   libfaketime and runs the ignored
//!   `rr-core/tests/realclock_wall_step.rs` test binary with it preloaded
//!   (`FAKETIME_DONT_FAKE_MONOTONIC=1`, `FAKETIME_NO_CACHE=1`, a timestamp
//!   file shared with the test via `RR_FAKETIME_FILE`), demands that exactly
//!   that test ran and passed, can't be skipped, and is in `ci-ok.needs`.
//!
//! The std-only ci.yml reader is shared with `t6_ci_config.rs`
//! (`common/workflow.rs`).

#[path = "common/workflow.rs"]
mod workflow;

use workflow::{
    GATE_JOB, Job, Workflow, assert_flags, ci_yml, gate_needs_problems, normalize,
    skippable_problems, trigger_problems,
};
use xtask::gates_selftest::Gate;

fn words(s: &str) -> Vec<&str> {
    s.split(' ').collect()
}

// --- US-RR-2-2: cargo-deny --------------------------------------------------

const DENY_JOB: &str = "deny";

/// The CI cargo-deny command: every check (no `check licenses` subset).
const DENY_CMD: &str = "cargo deny --locked check";

/// Every way `src` fails the US-RR-2-2 wiring; empty when it holds.
fn deny_problems(src: &str) -> Vec<String> {
    let wf = Workflow::parse(src);
    let mut problems = trigger_problems(&wf);
    problems.extend(gate_needs_problems(&wf, DENY_JOB));
    let Some(job) = wf.job(DENY_JOB) else {
        problems.push(format!("no `{DENY_JOB}` job"));
        return problems;
    };
    let scripts = job.run_scripts();
    if !scripts.iter().any(|s| s == DENY_CMD) {
        problems.push(format!(
            "`{DENY_JOB}` does not run `{DENY_CMD}` (scripts: {scripts:?})"
        ));
    }
    if !scripts.iter().any(|s| s == "cargo xtask deny-selftest") {
        problems.push(format!(
            "`{DENY_JOB}` does not run `cargo xtask deny-selftest`"
        ));
    }
    problems.extend(skippable_problems(job, &[]));
    problems
}

#[test]
fn ci_deny_job_runs_cargo_deny_and_selftest_as_gate() {
    let problems = deny_problems(&ci_yml());
    assert!(
        problems.is_empty(),
        ".github/workflows/ci.yml violates US-RR-2-2:\n{}",
        problems.join("\n")
    );
}

#[test]
fn ci_deny_check_rejects_weakened_deny_job() {
    let m = |edit: &dyn Fn(&str) -> String| workflow::mutated(deny_problems, edit);
    assert_flags(
        &m(&|s| {
            s.replace(
                "run: cargo deny --locked check\n",
                "run: cargo deny --locked check licenses\n",
            )
        }),
        "does not run `cargo deny --locked check`",
    );
    assert_flags(
        &m(&|s| {
            s.replace(
                "run: cargo deny --locked check\n",
                "run: cargo deny check\n",
            )
        }),
        "does not run `cargo deny --locked check`",
    );
    assert_flags(
        &m(&|s| s.replace("run: cargo xtask deny-selftest", "run: echo skipped")),
        "does not run `cargo xtask deny-selftest`",
    );
    assert_flags(
        &m(&|s| {
            s.replace(
                "      - name: cargo deny check\n",
                "      - name: cargo deny check\n        continue-on-error: true\n",
            )
        }),
        "continue-on-error",
    );
    assert_flags(
        &m(&|s| {
            s.replace(
                "      - name: cargo deny check\n",
                "      - name: cargo deny check\n        if: false\n",
            )
        }),
        "has a conditional",
    );
    assert_flags(
        &m(&|s| {
            s.replace(
                "run: cargo deny --locked check\n",
                "run: cargo deny --locked check || true\n",
            )
        }),
        "swallows failures",
    );
    assert_flags(
        &m(&|s| {
            s.replace(
                "needs: [fmt, clippy, test, deny, ",
                "needs: [fmt, clippy, test, ",
            )
        }),
        "`ci-ok.needs` does not include `deny`",
    );
    assert_flags(
        &m(&|s| s.replace("\n  deny:\n", "\n  supply-chain:\n")),
        "no `deny` job",
    );
}

// --- US-RR-2-3: pending insta snapshots -------------------------------------

const SNAP_JOB: &str = "snapshots";

/// Every way `src` fails the US-RR-2-3 wiring; empty when it holds.
fn snapshots_problems(src: &str) -> Vec<String> {
    let wf = Workflow::parse(src);
    let mut problems = trigger_problems(&wf);
    problems.extend(gate_needs_problems(&wf, SNAP_JOB));

    // INSTA_UPDATE=no at workflow level (every job) or on the job itself.
    let insta_no = |lines: &[&str]| {
        lines
            .iter()
            .any(|l| normalize(l).replace(['"', '\''], "") == "INSTA_UPDATE: no")
    };
    let Some(job) = wf.job(SNAP_JOB) else {
        problems.push(format!("no `{SNAP_JOB}` job"));
        return problems;
    };
    if !insta_no(wf.section("env")) && !insta_no(&job.lines) {
        problems.push("`INSTA_UPDATE: \"no\"` is not set for the snapshots job".to_owned());
    }

    let steps = job.steps();
    let script_of = |step: &Job<'_>| step.run_scripts().join(" ");
    let fails = |s: &str| s.contains("exit 1");
    let names_pending = |s: &str| s.contains("'*.snap.new'") && s.contains("'*.pending-snap'");

    let committed = steps.iter().position(|st| {
        let s = script_of(st);
        s.contains("git ls-files") && names_pending(&s) && fails(&s)
    });
    let insta = steps.iter().position(|st| {
        st.run_scripts().iter().any(|s| {
            let w = words(s);
            s.starts_with("cargo insta test")
                && ["--workspace", "--check", "--unreferenced=reject"]
                    .iter()
                    .all(|f| w.contains(f))
                && !["--accept", "--review", "--force-update-snapshots"]
                    .iter()
                    .any(|f| w.contains(f))
        })
    });
    let left_behind = steps.iter().position(|st| {
        let s = script_of(st);
        s.contains("find ")
            && s.contains("'*.snap.new'")
            && s.contains("'*.pending-snap'")
            && fails(&s)
    });

    match (committed, insta) {
        (None, _) => problems.push(format!(
            "`{SNAP_JOB}` has no `git ls-files` step failing on committed '*.snap.new' / '*.pending-snap'"
        )),
        (Some(c), Some(i)) if c > i => problems.push(format!(
            "`{SNAP_JOB}` checks for committed pending snapshots after the tests ran (insta deletes them)"
        )),
        _ => {}
    }
    if insta.is_none() {
        problems.push(format!(
            "`{SNAP_JOB}` does not run `cargo insta test --workspace --check --unreferenced=reject`"
        ));
    }
    match left_behind {
        None => problems.push(format!(
            "`{SNAP_JOB}` has no final `find` step failing on left-behind pending snapshots"
        )),
        Some(f) => {
            let always = steps[f]
                .content_lines()
                .any(|l| normalize(l) == "if: always()");
            if !always {
                problems.push(format!(
                    "`{SNAP_JOB}` left-behind check is not `if: always()`"
                ));
            }
            if insta.is_some_and(|i| f < i) {
                problems.push(format!(
                    "`{SNAP_JOB}` left-behind check runs before the tests"
                ));
            }
        }
    }
    let allowed: Vec<&str> = left_behind.map(|f| steps[f].id).into_iter().collect();
    problems.extend(skippable_problems(job, &allowed));
    problems
}

#[test]
fn ci_snapshots_job_fails_on_pending_snapshots() {
    let problems = snapshots_problems(&ci_yml());
    assert!(
        problems.is_empty(),
        ".github/workflows/ci.yml violates US-RR-2-3:\n{}",
        problems.join("\n")
    );
}

#[test]
fn ci_snapshots_check_rejects_weakened_snapshots_job() {
    let m = |edit: &dyn Fn(&str) -> String| workflow::mutated(snapshots_problems, edit);
    assert_flags(
        &m(&|s| {
            s.replace(
                "pending=$(git ls-files -- '*.snap.new' '*.pending-snap')",
                "pending=",
            )
        }),
        "no `git ls-files` step",
    );
    assert_flags(
        &m(&|s| s.replacen(" --unreferenced=reject", "", 1)),
        "does not run `cargo insta test",
    );
    assert_flags(
        &m(&|s| {
            s.replacen(
                "cargo insta test --workspace --check",
                "cargo insta test --workspace --accept",
                1,
            )
        }),
        "does not run `cargo insta test",
    );
    assert_flags(
        &m(&|s| s.replace("  INSTA_UPDATE: \"no\"\n", "")),
        "INSTA_UPDATE",
    );
    assert_flags(
        &m(&|s| {
            s.replace(
                "      - name: No pending snapshots left behind\n        if: always()\n",
                "      - name: No pending snapshots left behind\n",
            )
        }),
        "is not `if: always()`",
    );
    assert_flags(
        &m(&|s| {
            s.replace(
                "left=$(find . -path ./target -prune -o \\( -name '*.snap.new' -o -name '*.pending-snap' \\) -print)",
                "left=",
            )
        }),
        "no final `find` step",
    );
    assert_flags(
        &m(&|s| {
            s.replace(
                "      - name: No pending snapshots committed\n",
                "      - name: No pending snapshots committed\n        if: false\n",
            )
        }),
        "has a conditional",
    );
    assert_flags(
        &m(&|s| {
            s.replace(
                "needs: [fmt, clippy, test, deny, t6, snapshots, ",
                "needs: [fmt, clippy, test, deny, t6, ",
            )
        }),
        "`ci-ok.needs` does not include `snapshots`",
    );
}

/// Moving the committed-pending check after the tests is caught (insta would
/// already have deleted a committed stale `.snap.new`).
#[test]
fn ci_snapshots_check_rejects_committed_check_after_tests() {
    let src = ci_yml();
    let start = src
        .find("      - name: No pending snapshots committed\n")
        .unwrap_or(0);
    let end = src[start..]
        .find("      - name: Install toolchain\n")
        .map_or(start, |e| start + e);
    let block = &src[start..end];
    assert!(!block.is_empty(), "committed-pending step not found");
    let anchor = "      - name: No pending snapshots left behind\n";
    let moved = src
        .replacen(block, "", 1)
        .replacen(anchor, &format!("{block}{anchor}"), 1);
    assert_ne!(moved, src);
    assert_flags(&snapshots_problems(&moved), "after the tests ran");
}

// --- US-RR-2-4: i686-pc-windows-msvc -----------------------------------------

const TEST_JOB: &str = "test";
const I686: &str = "i686-pc-windows-msvc";

/// The `strategy.matrix.include` entries of a job, as key/value lists.
fn matrix_include(job: &Job<'_>) -> Vec<Vec<(String, String)>> {
    let Some(start) = job.lines.iter().position(|l| l.trim() == "include:") else {
        return Vec::new();
    };
    let base = workflow::indent(job.lines[start]);
    let mut entries: Vec<Vec<(String, String)>> = Vec::new();
    for &line in &job.lines[start + 1..] {
        if workflow::is_blank_or_comment(line) {
            continue;
        }
        if workflow::indent(line) <= base {
            break;
        }
        let t = line.trim();
        let t = if let Some(rest) = t.strip_prefix("- ") {
            entries.push(Vec::new());
            rest
        } else {
            t
        };
        if let (Some(entry), Some((k, v))) = (entries.last_mut(), t.split_once(':')) {
            entry.push((
                k.trim().to_owned(),
                v.trim().trim_matches(|c| c == '"' || c == '\'').to_owned(),
            ));
        }
    }
    entries
}

/// `text` with `${{ matrix.<key> }}` replaced from `entry`.
fn expand(text: &str, entry: &[(String, String)]) -> String {
    entry.iter().fold(text.to_owned(), |t, (k, v)| {
        t.replace(&format!("${{{{ matrix.{k} }}}}"), v)
    })
}

/// A predicate over one normalized, matrix-expanded `run:` script.
type StepPred<'a> = dyn Fn(&str) -> bool + 'a;

/// Every way `src` fails the US-RR-2-4 wiring; empty when it holds.
fn i686_problems(src: &str) -> Vec<String> {
    let wf = Workflow::parse(src);
    let mut problems = trigger_problems(&wf);
    problems.extend(gate_needs_problems(&wf, TEST_JOB));
    let Some(job) = wf.job(TEST_JOB) else {
        problems.push(format!("no `{TEST_JOB}` job"));
        return problems;
    };
    let include = matrix_include(job);
    let Some(entry) = include
        .iter()
        .find(|e| e.iter().any(|(k, v)| k == "target" && v == I686))
    else {
        problems.push(format!("`{TEST_JOB}` matrix has no `target: {I686}` entry"));
        return problems;
    };
    if job.lines.iter().any(|l| l.trim() == "exclude:") {
        problems.push(format!("`{TEST_JOB}` matrix has an `exclude:` list"));
    }
    let runs_on = job
        .value_at("runs-on", 4)
        .map(|r| expand(&r, entry))
        .unwrap_or_default();
    if !runs_on.starts_with("windows-") {
        problems.push(format!(
            "`{TEST_JOB}` ({I686}) does not run on a Windows runner (runs-on: {runs_on:?})"
        ));
    }
    // Each required command must be in a step that always runs: other steps
    // of the job (e.g. host-only self-tests) may carry their own `if:`.
    let steps = job.steps();
    let targets_i686 = |w: &[&str]| w.windows(2).any(|p| p == ["--target", I686]);
    let add_target = format!("rustup target add {I686}");
    let required: [(&str, &StepPred<'_>); 3] = [
        (&format!("`rustup target add {I686}`"), &|s: &str| {
            s.contains(&add_target)
        }),
        (
            &format!("`cargo nextest run --workspace --target {I686}`"),
            &|s: &str| {
                let w = words(s);
                w.starts_with(&["cargo", "nextest", "run"])
                    && w.contains(&"--workspace")
                    && targets_i686(&w)
            },
        ),
        (
            &format!("the doctests with `--target {I686}`"),
            &|s: &str| {
                let w = words(s);
                w.starts_with(&["cargo", "test", "--doc"])
                    && w.contains(&"--workspace")
                    && targets_i686(&w)
            },
        ),
    ];
    for (what, pred) in required {
        let step = steps
            .iter()
            .find(|st| st.run_scripts().iter().any(|s| pred(&expand(s, entry))));
        match step {
            None => problems.push(format!("`{TEST_JOB}` ({I686}) does not run {what}")),
            Some(st) => {
                for l in st.content_lines() {
                    let l = l.strip_prefix("- ").unwrap_or(l);
                    if l.starts_with("if:") || l.starts_with("continue-on-error") {
                        problems.push(format!(
                            "`{TEST_JOB}` step `{}` (runs {what}) has `{l}`",
                            st.id
                        ));
                    }
                }
                for s in st.run_scripts() {
                    if s.contains("|| true") || s.contains("|| :") || s.contains("exit 0") {
                        problems.push(format!("`{TEST_JOB}` step swallows failures: {s}"));
                    }
                }
            }
        }
    }
    if job
        .content_lines()
        .any(|l| l.starts_with("continue-on-error"))
    {
        problems.push(format!("`{TEST_JOB}` uses continue-on-error"));
    }
    if job.value_at("if", 4).is_some() {
        problems.push(format!("`{TEST_JOB}` job has a job-level `if:`"));
    }
    problems
}

#[test]
fn ci_test_matrix_builds_and_tests_i686_windows() {
    let problems = i686_problems(&ci_yml());
    assert!(
        problems.is_empty(),
        ".github/workflows/ci.yml violates US-RR-2-4:\n{}",
        problems.join("\n")
    );
}

#[test]
fn ci_i686_check_rejects_weakened_matrix() {
    let m = |edit: &dyn Fn(&str) -> String| workflow::mutated(i686_problems, edit);
    assert_flags(
        &m(&|s| {
            s.replace(
                "- target: i686-pc-windows-msvc",
                "- target: x86_64-pc-windows-gnu",
            )
        }),
        "no `target: i686-pc-windows-msvc` entry",
    );
    assert_flags(
        &m(&|s| {
            s.replace(
                "          - target: i686-pc-windows-msvc\n            os: windows-2022\n",
                "          - target: i686-pc-windows-msvc\n            os: ubuntu-24.04\n",
            )
        }),
        "does not run on a Windows runner",
    );
    assert_flags(
        &m(&|s| {
            s.replace(
                "cargo nextest run --workspace --locked --target ${{ matrix.target }}",
                "cargo nextest run --workspace --locked",
            )
        }),
        "does not run `cargo nextest run --workspace --target",
    );
    assert_flags(
        &m(&|s| s.replace("rustup target add ${{ matrix.target }}", "true")),
        "does not run `rustup target add",
    );
    assert_flags(
        &m(&|s| {
            s.replace(
                "      - name: cargo nextest\n",
                "      - name: cargo nextest\n        if: matrix.target != 'i686-pc-windows-msvc'\n",
            )
        }),
        "has `if: matrix.target",
    );
    assert_flags(
        &m(&|s| {
            s.replace(
                "      - name: doctests\n",
                "      - name: doctests\n        continue-on-error: true\n",
            )
        }),
        "has `continue-on-error",
    );
    assert_flags(
        &m(&|s| {
            s.replace(
                "    needs: [fmt, clippy, test, ",
                "    needs: [fmt, clippy, ",
            )
        }),
        "`ci-ok.needs` does not include `test`",
    );
}

// --- US-RR-2-1: fmt / clippy / nextest block a PR on every matrix target --

const FMT_JOB: &str = "fmt";
const CLIPPY_JOB: &str = "clippy";

/// The story's tier-1/2 targets (US-RR-2: "nextest on x86_64-linux,
/// aarch64-linux, x86_64-windows-msvc, i686-windows-msvc").
const MATRIX_TARGETS: [&str; 4] = [
    "x86_64-unknown-linux-gnu",
    "aarch64-unknown-linux-gnu",
    "x86_64-pc-windows-msvc",
    I686,
];

/// The `ci-ok` failing step's condition terms: any needed job not `success`.
const GATE_FAIL_RESULTS: [&str; 3] = ["failure", "cancelled", "skipped"];

/// Problems if `job` has a job-level `if:` (a skipped job is not a pass, but
/// it must not be conditional either).
fn job_if_problems(job: &Job<'_>) -> Vec<String> {
    job.value_at("if", 4)
        .map(|v| format!("`{}` has a job-level `if: {v}`", job.id))
        .into_iter()
        .collect()
}

/// Problems if no step of `job` runs exactly `cmd` (after matrix expansion
/// with `entry`).
fn runs_problems(job: &Job<'_>, entry: &[(String, String)], cmd: &str, label: &str) -> Vec<String> {
    let found = job
        .steps()
        .iter()
        .any(|st| st.run_scripts().iter().any(|s| expand(s, entry) == cmd));
    if found {
        Vec::new()
    } else {
        vec![format!("`{}`{label} does not run `{cmd}`", job.id)]
    }
}

/// Problems with the aggregate gate's own logic: it must run even when a
/// needed job failed (`if: always()`) and must fail when any needed job's
/// result is `failure`, `cancelled` or `skipped`.
fn gate_logic_problems(wf: &Workflow<'_>) -> Vec<String> {
    let Some(gate) = wf.job(GATE_JOB) else {
        return vec![format!("no `{GATE_JOB}` job")];
    };
    let mut problems = Vec::new();
    if gate.value_at("if", 4).map(|v| normalize(&v)).as_deref() != Some("always()") {
        problems.push(format!(
            "`{GATE_JOB}` is not `if: always()`, so a failed gate skips it (a skipped required check counts as passed)"
        ));
    }
    if gate
        .content_lines()
        .any(|l| l.trim_start_matches("- ").starts_with("continue-on-error"))
    {
        problems.push(format!("`{GATE_JOB}` uses continue-on-error"));
    }
    let steps = gate.steps();
    let failing: Vec<&Job<'_>> = steps
        .iter()
        .filter(|st| st.run_scripts().iter().any(|s| s.contains("exit 1")))
        .collect();
    if failing.is_empty() {
        problems.push(format!("`{GATE_JOB}` has no failing (`exit 1`) step"));
    }
    for st in failing {
        for s in st.run_scripts() {
            if s.contains("|| true") || s.contains("|| :") || s.contains("exit 0") {
                problems.push(format!("`{GATE_JOB}` failing step swallows failures: {s}"));
            }
        }
        // No `if:` means the step always fails: never a weakening.
        let Some(cond) = st.value_at("if", 8) else {
            continue;
        };
        let inner = cond
            .trim()
            .strip_prefix("${{")
            .and_then(|c| c.strip_suffix("}}"))
            .unwrap_or(&cond);
        let terms: Vec<String> = inner.split("||").map(normalize).collect();
        let wanted: Vec<String> = GATE_FAIL_RESULTS
            .iter()
            .map(|r| format!("contains(needs.*.result, '{r}')"))
            .collect();
        for (r, w) in GATE_FAIL_RESULTS.iter().zip(&wanted) {
            if !terms.contains(w) {
                problems.push(format!(
                    "`{GATE_JOB}` does not fail on a needed job's `{r}` result (if: {cond})"
                ));
            }
        }
        for t in terms.iter().filter(|t| !wanted.contains(t)) {
            problems.push(format!(
                "`{GATE_JOB}` failing step has an unexpected condition term `{t}`"
            ));
        }
    }
    problems
}

/// Every way `src` fails the US-RR-2-1 wiring; empty when it holds.
fn pr_gates_problems(src: &str) -> Vec<String> {
    let wf = Workflow::parse(src);
    let mut problems = trigger_problems(&wf);
    if !wf
        .section("defaults")
        .iter()
        .any(|l| normalize(l) == "shell: bash")
    {
        problems.push(
            "`defaults.run.shell` is not `bash`, so a failing command in a multi-line `run:` may not fail its step"
                .to_owned(),
        );
    }
    problems.extend(gate_logic_problems(&wf));

    for (id, gate) in [(FMT_JOB, Gate::Fmt), (CLIPPY_JOB, Gate::Clippy)] {
        problems.extend(gate_needs_problems(&wf, id));
        let Some(job) = wf.job(id) else {
            problems.push(format!("no `{id}` job"));
            continue;
        };
        problems.extend(runs_problems(job, &[], &gate.ci_command(None), ""));
        problems.extend(skippable_problems(job, &[]));
        problems.extend(job_if_problems(job));
    }

    problems.extend(gate_needs_problems(&wf, TEST_JOB));
    let Some(job) = wf.job(TEST_JOB) else {
        problems.push(format!("no `{TEST_JOB}` job"));
        return problems;
    };
    problems.extend(skippable_problems(job, &[]));
    problems.extend(job_if_problems(job));
    if job.value_at("fail-fast", 6).as_deref() != Some("false") {
        problems.push(format!(
            "`{TEST_JOB}` matrix is not `fail-fast: false`, so one target's failure hides the others' results"
        ));
    }
    if job.lines.iter().any(|l| l.trim() == "exclude:") {
        problems.push(format!("`{TEST_JOB}` matrix has an `exclude:` list"));
    }
    let rustflags: Vec<String> = job
        .content_lines()
        .filter(|l| l.starts_with("RUSTFLAGS:"))
        .map(|l| normalize(l).replace(['"', '\''], ""))
        .collect();
    if rustflags != ["RUSTFLAGS: -D warnings"] {
        problems.push(format!(
            "`{TEST_JOB}` does not set `RUSTFLAGS: -D warnings` exactly once (found {rustflags:?})"
        ));
    }
    let include = matrix_include(job);
    for t in MATRIX_TARGETS {
        if !include
            .iter()
            .any(|e| e.iter().any(|(k, v)| k == "target" && v == t))
        {
            problems.push(format!("`{TEST_JOB}` matrix has no `target: {t}` entry"));
        }
    }
    for entry in &include {
        let Some((_, t)) = entry.iter().find(|(k, _)| k == "target") else {
            problems.push(format!(
                "`{TEST_JOB}` matrix entry without a target: {entry:?}"
            ));
            continue;
        };
        let label = format!(" ({t})");
        for cmd in [
            Gate::Clippy.ci_command(Some(t)),
            Gate::Nextest.ci_command(Some(t)),
            format!("cargo xtask gates-selftest --target {t}"),
        ] {
            problems.extend(runs_problems(job, entry, &cmd, &label));
        }
    }
    problems
}

#[test]
fn ci_blocks_fmt_clippy_and_test_failures_on_every_matrix_target() {
    let problems = pr_gates_problems(&ci_yml());
    assert!(
        problems.is_empty(),
        ".github/workflows/ci.yml violates US-RR-2-1:\n{}",
        problems.join("\n")
    );
}

#[test]
fn ci_pr_gates_check_rejects_weakened_fmt_and_clippy_jobs() {
    let m = |edit: &dyn Fn(&str) -> String| workflow::mutated(pr_gates_problems, edit);
    assert_flags(
        &m(&|s| s.replace("run: cargo fmt --all --check\n", "run: cargo fmt --all\n")),
        "`fmt` does not run `cargo fmt --all --check`",
    );
    assert_flags(
        &m(&|s| {
            s.replace(
                "      - name: cargo fmt --check\n",
                "      - name: cargo fmt --check\n        continue-on-error: true\n",
            )
        }),
        "continue-on-error",
    );
    assert_flags(
        &m(&|s| {
            s.replace(
                "    name: fmt\n",
                "    name: fmt\n    if: github.event_name == 'push'\n",
            )
        }),
        "`fmt` has a job-level `if:",
    );
    assert_flags(
        &m(&|s| s.replace("needs: [fmt, clippy, ", "needs: [clippy, ")),
        "`ci-ok.needs` does not include `fmt`",
    );
    assert_flags(
        &m(&|s| s.replace("\n  fmt:\n", "\n  format:\n")),
        "no `fmt` job",
    );
    assert_flags(
        &m(&|s| {
            s.replace(
                "run: cargo clippy --workspace --all-targets --locked -- -D warnings\n",
                "run: cargo clippy --workspace --all-targets --locked\n",
            )
        }),
        "`clippy` does not run `cargo clippy --workspace --all-targets --locked -- -D warnings`",
    );
    assert_flags(
        &m(&|s| {
            s.replace(
                "run: cargo clippy --workspace --all-targets --locked -- -D warnings\n",
                "run: cargo clippy --workspace --all-targets --locked -- -D warnings || true\n",
            )
        }),
        "swallows failures",
    );
    assert_flags(
        &m(&|s| {
            s.replace(
                "      - name: cargo clippy\n",
                "      - name: cargo clippy\n        if: github.event_name == 'push'\n",
            )
        }),
        "`clippy` step `cargo clippy` has a conditional",
    );
    assert_flags(
        &m(&|s| s.replace("needs: [fmt, clippy, ", "needs: [fmt, ")),
        "`ci-ok.needs` does not include `clippy`",
    );
    assert_flags(
        &m(&|s| s.replace("    shell: bash\n", "    shell: pwsh\n")),
        "`defaults.run.shell` is not `bash`",
    );
}

#[test]
fn ci_pr_gates_check_rejects_weakened_test_matrix() {
    let m = |edit: &dyn Fn(&str) -> String| workflow::mutated(pr_gates_problems, edit);
    assert_flags(
        &m(&|s| {
            s.replace(
                "          - target: aarch64-unknown-linux-gnu\n            os: ubuntu-24.04-arm\n",
                "",
            )
        }),
        "matrix has no `target: aarch64-unknown-linux-gnu` entry",
    );
    assert_flags(
        &m(&|s| s.replace("      fail-fast: false\n", "      fail-fast: true\n")),
        "not `fail-fast: false`",
    );
    assert_flags(
        &m(&|s| {
            s.replace(
                "      matrix:\n        include:\n",
                "      matrix:\n        exclude:\n          - target: x86_64-pc-windows-msvc\n        include:\n",
            )
        }),
        "has an `exclude:` list",
    );
    assert_flags(
        &m(&|s| s.replace("      RUSTFLAGS: -D warnings\n", "")),
        "RUSTFLAGS: -D warnings",
    );
    assert_flags(
        &m(&|s| {
            s.replace(
                "      - name: cargo nextest\n",
                "      - name: cargo nextest\n        env:\n          RUSTFLAGS: \"\"\n",
            )
        }),
        "RUSTFLAGS: -D warnings",
    );
    // Per-target clippy: dropped, host-only, or conditional on the target.
    let clippy_target = "cargo clippy --workspace --all-targets --locked --target ${{ matrix.target }} -- -D warnings";
    for t in MATRIX_TARGETS {
        assert_flags(
            &m(&|s| {
                s.replace(
                    clippy_target,
                    "cargo clippy --workspace --all-targets --locked -- -D warnings",
                )
            }),
            &format!(
                "`test` ({t}) does not run `cargo clippy --workspace --all-targets --locked --target {t} -- -D warnings`"
            ),
        );
        assert_flags(
            &m(&|s| {
                s.replace("cargo nextest run --workspace --locked --target ${{ matrix.target }}", "cargo nextest run --workspace --locked --target ${{ matrix.target }} -E 'not test(/slow/)'")
            }),
            &format!(
                "`test` ({t}) does not run `cargo nextest run --workspace --locked --target {t}`"
            ),
        );
        assert_flags(
            &m(&|s| {
                s.replace(
                    "cargo xtask gates-selftest --target ${{ matrix.target }}",
                    "cargo xtask gates-selftest",
                )
            }),
            &format!("`test` ({t}) does not run `cargo xtask gates-selftest --target {t}`"),
        );
    }
    assert_flags(
        &m(&|s| {
            s.replace(
                "      - name: cargo clippy (target)\n",
                "      - name: cargo clippy (target)\n        if: runner.os == 'Linux'\n",
            )
        }),
        "`test` step `cargo clippy (target)` has a conditional",
    );
    assert_flags(
        &m(&|s| {
            s.replace(
                "      - name: cargo nextest\n",
                "      - name: cargo nextest\n        continue-on-error: ${{ matrix.target == 'i686-pc-windows-msvc' }}\n",
            )
        }),
        "continue-on-error",
    );
    assert_flags(
        &m(&|s| {
            s.replace(
                "    name: test (${{ matrix.target }})\n",
                "    name: test (${{ matrix.target }})\n    continue-on-error: true\n",
            )
        }),
        "continue-on-error",
    );
    assert_flags(
        &m(&|s| {
            s.replace(
                "run: cargo nextest run --workspace --locked --target ${{ matrix.target }}\n",
                "run: cargo nextest run --workspace --locked --target ${{ matrix.target }} || true\n",
            )
        }),
        "swallows failures",
    );
    assert_flags(
        &m(&|s| {
            s.replace(
                "      - name: cargo xtask gates-selftest\n",
                "      - name: cargo xtask gates-selftest\n        if: false\n",
            )
        }),
        "`test` step `cargo xtask gates-selftest` has a conditional",
    );
    assert_flags(
        &m(&|s| s.replace("needs: [fmt, clippy, test, ", "needs: [fmt, clippy, ")),
        "`ci-ok.needs` does not include `test`",
    );
}

#[test]
fn ci_pr_gates_check_rejects_weakened_ci_ok() {
    let m = |edit: &dyn Fn(&str) -> String| workflow::mutated(pr_gates_problems, edit);
    let fail_if = "        if: ${{ contains(needs.*.result, 'failure') || contains(needs.*.result, 'cancelled') || contains(needs.*.result, 'skipped') }}\n";
    assert_flags(
        &m(&|s| s.replace("    name: ci-ok\n    if: always()\n", "    name: ci-ok\n")),
        "is not `if: always()`",
    );
    for r in GATE_FAIL_RESULTS {
        assert_flags(
            &m(&|s| {
                let weakened = fail_if
                    .replace(&format!(" || contains(needs.*.result, '{r}')"), "")
                    .replace(&format!("contains(needs.*.result, '{r}') || "), "");
                s.replace(fail_if, &weakened)
            }),
            &format!("does not fail on a needed job's `{r}` result"),
        );
    }
    assert_flags(
        &m(&|s| s.replace(fail_if, &fail_if.replace(" }}", " && false }}"))),
        "unexpected condition term",
    );
    assert_flags(
        &m(&|s| {
            s.replace(
                "join(needs.*.result, ', ') }}\"\n          exit 1\n",
                "join(needs.*.result, ', ') }}\"\n          exit 0\n",
            )
        }),
        "has no failing (`exit 1`) step",
    );
    assert_flags(
        &m(&|s| {
            s.replace(
                "      - name: A gate failed, was cancelled, or was skipped\n",
                "      - name: A gate failed, was cancelled, or was skipped\n        continue-on-error: true\n",
            )
        }),
        "`ci-ok` uses continue-on-error",
    );
}

#[test]
fn matrix_reader_reads_include_entries() {
    let src = "jobs:\n  test:\n    strategy:\n      matrix:\n        include:\n          - target: a\n            os: x\n          # c\n          - os: \"y\"\n            target: b\n    steps:\n      - run: echo\n";
    let wf = Workflow::parse(src);
    let inc = wf.job("test").map(matrix_include).unwrap_or_default();
    let kv = |k: &str, v: &str| (k.to_owned(), v.to_owned());
    assert_eq!(
        inc,
        vec![
            vec![kv("target", "a"), kv("os", "x")],
            vec![kv("os", "y"), kv("target", "b")]
        ]
    );
    assert_eq!(
        expand("run on ${{ matrix.os }} for ${{ matrix.target }}", &inc[1]),
        "run on y for b"
    );
}

// --- US-RR-18-3: RealClock under an OS wall-clock step ----------------------

const FAKETIME_JOB: &str = "faketime";
const WALL_STEP_TEST: &str = "real_clock_mono_never_follows_os_wall_clock_steps";

/// What the wall-step run needs: libfaketime preloaded on the test binary,
/// monotonic time left real, the timestamp file re-read on every call and
/// shared with the test, the ignored test selected, and a guard that it ran.
const WALL_STEP_NEEDS: &[&str] = &[
    "LD_PRELOAD=/usr/lib/x86_64-linux-gnu/faketime/libfaketime.so.1",
    "FAKETIME_DONT_FAKE_MONOTONIC=1",
    "FAKETIME_NO_CACHE=1",
    "FAKETIME_TIMESTAMP_FILE=\"$ts\"",
    "RR_FAKETIME_FILE=\"$ts\"",
    "--ignored --exact",
    "grep -q '^test result: ok. 1 passed'",
];

/// The committed wall-step test file, `\r\n` normalised like [`ci_yml`].
fn wall_step_test_rs() -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../crates/rr-core/tests/realclock_wall_step.rs");
    std::fs::read_to_string(&path)
        .map(|s| s.replace("\r\n", "\n"))
        .unwrap_or_else(|e| format!("<unreadable {}: {e}>", path.display()))
}

/// Every way `src` (ci.yml) and `test_rs` fail the US-RR-18-3 wiring.
fn faketime_problems_in(src: &str, test_rs: &str) -> Vec<String> {
    let wf = Workflow::parse(src);
    let mut problems = trigger_problems(&wf);
    problems.extend(gate_needs_problems(&wf, FAKETIME_JOB));
    if !test_rs.contains(&format!("fn {WALL_STEP_TEST}()")) {
        problems.push(format!("realclock_wall_step.rs has no `{WALL_STEP_TEST}`"));
    }
    let Some(job) = wf.job(FAKETIME_JOB) else {
        problems.push(format!("no `{FAKETIME_JOB}` job"));
        return problems;
    };
    let scripts = job.run_scripts();
    if !scripts
        .iter()
        .any(|s| s.contains("apt-get install") && s.contains("libfaketime"))
    {
        problems.push(format!("`{FAKETIME_JOB}` does not install libfaketime"));
    }
    match scripts.iter().find(|s| s.contains(WALL_STEP_TEST)) {
        None => problems.push(format!("`{FAKETIME_JOB}` does not run `{WALL_STEP_TEST}`")),
        Some(run) => {
            for needed in WALL_STEP_NEEDS {
                if !run.contains(needed) {
                    problems.push(format!("`{FAKETIME_JOB}` wall-step run lacks `{needed}`"));
                }
            }
        }
    }
    problems.extend(skippable_problems(job, &[]));
    problems
}

fn faketime_problems(src: &str) -> Vec<String> {
    faketime_problems_in(src, &wall_step_test_rs())
}

#[test]
fn ci_faketime_job_steps_the_wall_clock_under_real_clock_as_gate() {
    let problems = faketime_problems(&ci_yml());
    assert!(
        problems.is_empty(),
        ".github/workflows/ci.yml violates US-RR-18-3:\n{}",
        problems.join("\n")
    );
}

#[test]
fn ci_faketime_check_rejects_weakened_job() {
    let m = |edit: &dyn Fn(&str) -> String| workflow::mutated(faketime_problems, edit);
    for needed in WALL_STEP_NEEDS {
        assert_flags(&m(&|s| s.replace(needed, "")), needed);
    }
    assert_flags(
        &m(&|s| s.replace("--no-install-recommends libfaketime", "jq")),
        "does not install libfaketime",
    );
    assert_flags(
        &m(&|s| s.replace(&format!("--exact {WALL_STEP_TEST}"), "--exact")),
        "does not run",
    );
    assert_flags(
        &m(&|s| {
            s.replace(
                "      - name: RealClock under wall-clock steps\n",
                "      - name: RealClock under wall-clock steps\n        continue-on-error: true\n",
            )
        }),
        "continue-on-error",
    );
    assert_flags(
        &m(&|s| {
            s.replace(
                "      - name: RealClock under wall-clock steps\n",
                "      - name: RealClock under wall-clock steps\n        if: false\n",
            )
        }),
        "has a conditional",
    );
    assert_flags(
        &m(&|s| s.replace(", snapshots, faketime]", ", snapshots]")),
        "`ci-ok.needs` does not include `faketime`",
    );
    assert_flags(
        &m(&|s| s.replace("\n  faketime:\n", "\n  wall-clock:\n")),
        "no `faketime` job",
    );
    // Renaming the test without the job would select nothing.
    let renamed = wall_step_test_rs().replace(WALL_STEP_TEST, "renamed_test");
    assert_flags(
        &faketime_problems_in(&ci_yml(), &renamed),
        "realclock_wall_step.rs has no",
    );
}
