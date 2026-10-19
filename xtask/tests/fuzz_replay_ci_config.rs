//! `ci.yml` wiring for story US-RR-5, task US-RR-5-7: committed fuzz crash
//! inputs (`fuzz/regressions/<target>/`) run as ordinary regression tests on
//! every push and pull request, on the stable toolchain, without nightly.
//!
//! [`replay_problems`] is a pure check over the text of
//! `.github/workflows/ci.yml`; one test runs it on the committed file and the
//! `*_rejects_*` test proves each assertion bites on a mutated copy. It
//! asserts that job `fuzz-regressions`:
//!
//! * exists, runs on Linux, has a timeout, and is in `ci-ok.needs` (so it is
//!   part of the one required check); the workflow runs on `push` and
//!   `pull_request`;
//! * runs exactly `cargo xtask fuzz replay` (no `--features`: a planted bug
//!   would make the committed toy inputs fail, and a feature flag here could
//!   only weaken or break the run), which is the stable `cargo test` of the
//!   fuzz crate (`xtask/tests/fuzz.rs` pins its arguments);
//! * never installs or selects nightly, and never calls `cargo fuzz`;
//! * cannot be skipped or softened (`if:`, `continue-on-error`, `|| true`).
//!
//! That the replay itself fails on a planted panic, hang and runaway memory,
//! and on a regression directory for an unknown target, is proved by the
//! self-test at the start of every run (`fuzz/tests/regressions.rs`); whether
//! the job is green can only be seen on GitHub.

#[path = "common/workflow.rs"]
mod workflow;

use workflow::{
    Workflow, assert_flags, ci_yml, gate_needs_problems, skippable_problems, trigger_problems,
};

const JOB: &str = "fuzz-regressions";
const REPLAY: &str = "cargo xtask fuzz replay";

/// Every way `src` (ci.yml) fails the US-RR-5-7 wiring; empty when it holds.
fn replay_problems(src: &str) -> Vec<String> {
    let wf = Workflow::parse(src);
    let mut problems = trigger_problems(&wf);
    problems.extend(gate_needs_problems(&wf, JOB));
    let Some(job) = wf.job(JOB) else {
        problems.push(format!("no `{JOB}` job"));
        return problems;
    };
    if !job
        .value_at("runs-on", 4)
        .is_some_and(|r| r.starts_with("ubuntu-"))
    {
        problems.push(format!("`{JOB}` does not run on an ubuntu- runner"));
    }
    if job.value_at("timeout-minutes", 4).is_none() {
        problems.push(format!(
            "`{JOB}` has no timeout-minutes (a hang must not hold the runner for 6 h)"
        ));
    }
    let scripts = job.run_scripts();
    if !scripts.iter().any(|s| s == REPLAY) {
        problems.push(format!(
            "`{JOB}` does not run exactly `{REPLAY}` (scripts: {scripts:?})"
        ));
    }
    for s in &scripts {
        if s.contains("nightly") {
            problems.push(format!("`{JOB}` uses nightly: {s}"));
        }
        if s.contains("cargo fuzz") || s.contains("fuzz run") {
            problems.push(format!("`{JOB}` runs the fuzzer; it must only replay: {s}"));
        }
        if s.contains("--features") {
            problems.push(format!("`{JOB}` passes --features: {s}"));
        }
    }
    if job
        .content_lines()
        .any(|l| l.contains("toolchain: nightly"))
    {
        problems.push(format!("`{JOB}` uses nightly via an action input"));
    }
    problems.extend(skippable_problems(job, &[]));
    problems
}

#[test]
fn ci_replays_committed_fuzz_regressions_on_stable_as_gate() {
    let problems = replay_problems(&ci_yml());
    assert!(
        problems.is_empty(),
        ".github/workflows/ci.yml violates US-RR-5-7:\n{}",
        problems.join("\n")
    );
}

#[test]
fn ci_fuzz_replay_check_rejects_weakened_job() {
    let m = |edit: &dyn Fn(&str) -> String| workflow::mutated(replay_problems, edit);
    let run = format!("        run: {REPLAY}\n");

    assert_flags(
        &m(&|s| s.replace(", fuzz-regressions]", "]")),
        "`ci-ok.needs` does not include `fuzz-regressions`",
    );
    assert_flags(
        &m(&|s| s.replace("\n  fuzz-regressions:\n", "\n  fuzz-replay:\n")),
        "no `fuzz-regressions` job",
    );
    assert_flags(
        &m(&|s| {
            s.replace(
                &run,
                "        run: cargo test --manifest-path fuzz/Cargo.toml\n",
            )
        }),
        "does not run exactly",
    );
    assert_flags(
        &m(&|s| {
            s.replace(
                &run,
                &format!("        run: {REPLAY} --features planted-panic\n"),
            )
        }),
        "passes --features",
    );
    assert_flags(
        &m(&|s| s.replace(&run, &format!("        run: {REPLAY} || true\n"))),
        "swallows failures",
    );
    assert_flags(
        &m(&|s| {
            s.replace(
                &run,
                &format!("        run: |\n          rustup toolchain install nightly\n          {REPLAY}\n"),
            )
        }),
        "uses nightly",
    );
    assert_flags(
        &m(&|s| {
            s.replace(
                &run,
                &format!(
                    "        run: |\n          {REPLAY}\n          cargo xtask fuzz run toy\n"
                ),
            )
        }),
        "runs the fuzzer",
    );
    assert_flags(
        &m(&|s| {
            s.replace(
                "      - name: cargo xtask fuzz replay\n",
                "      - name: cargo xtask fuzz replay\n        continue-on-error: true\n",
            )
        }),
        "continue-on-error",
    );
    assert_flags(
        &m(&|s| {
            s.replace(
                "      - name: cargo xtask fuzz replay\n",
                "      - name: cargo xtask fuzz replay\n        if: github.event_name == 'push'\n",
            )
        }),
        "has a conditional",
    );
    assert_flags(
        &m(&|s| {
            s.replace(
                "    name: fuzz regressions (stable replay)\n    runs-on: ubuntu-24.04\n    timeout-minutes: 20\n",
                "    name: fuzz regressions (stable replay)\n    runs-on: ubuntu-24.04\n",
            )
        }),
        "has no timeout-minutes",
    );
    assert_flags(
        &m(&|s| {
            s.replace(
                "    name: fuzz regressions (stable replay)\n    runs-on: ubuntu-24.04\n",
                "    name: fuzz regressions (stable replay)\n    runs-on: self-hosted\n",
            )
        }),
        "does not run on an ubuntu- runner",
    );
}
