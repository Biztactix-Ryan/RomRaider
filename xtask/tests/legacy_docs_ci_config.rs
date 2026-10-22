//! `ci.yml` wiring for story US-RR-12, task US-RR-12-1: the legacy behaviour
//! catalogue check runs on every push and pull request, as part of the one
//! required check.
//!
//! [`wiring_problems`] is a pure check over the text of
//! `.github/workflows/ci.yml`; one test runs it on the committed file and the
//! `*_rejects_*` test proves each assertion bites on a mutated copy. It
//! asserts that job `clippy`:
//!
//! * exists, runs on Linux, has no job-level `if:`, and is in `ci-ok.needs`;
//!   the workflow runs on `push` and `pull_request`;
//! * runs exactly `cargo xtask check-legacy-docs`: no `--root`, so the
//!   committed pages are checked against the checked-out `legacy/`;
//! * cannot skip or soften the step (`if:`, `continue-on-error`, `|| true`).
//!
//! What the check rejects is proved by `xtask/tests/legacy_docs.rs`.

#[path = "common/workflow.rs"]
mod workflow;

use workflow::{
    Workflow, assert_flags, ci_yml, gate_needs_problems, skippable_problems, trigger_problems,
};

const JOB: &str = "clippy";
const CMD: &str = "cargo xtask check-legacy-docs";

/// Every way `src` (ci.yml) fails to run the check as a blocking gate.
fn wiring_problems(src: &str) -> Vec<String> {
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
    if job.value_at("if", 4).is_some() {
        problems.push(format!("`{JOB}` has a job-level `if:`"));
    }
    let scripts = job.run_scripts();
    if !scripts.iter().any(|s| s == CMD) {
        problems.push(format!(
            "`{JOB}` does not run exactly `{CMD}` (scripts: {scripts:?})"
        ));
    }
    for s in scripts.iter().filter(|s| s.contains("check-legacy-docs")) {
        if s.contains("--root") {
            problems.push(format!("`{JOB}` passes --root: {s}"));
        }
    }
    problems.extend(skippable_problems(job, &[]));
    problems
}

#[test]
fn ci_runs_the_legacy_docs_check_as_a_gate() {
    let problems = wiring_problems(&ci_yml());
    assert!(
        problems.is_empty(),
        ".github/workflows/ci.yml does not gate on {CMD}:\n{}",
        problems.join("\n")
    );
}

#[test]
fn ci_legacy_docs_wiring_rejects_weakened_ci() {
    let m = |edit: &dyn Fn(&str) -> String| workflow::mutated(wiring_problems, edit);
    let step = format!("        run: {CMD}\n");
    assert_flags(
        &m(&|s| s.replace(&step, "        run: echo skipped\n")),
        "does not run exactly `cargo xtask check-legacy-docs`",
    );
    assert_flags(
        &m(&|s| s.replace(&step, &format!("        run: # {CMD}\n"))),
        "does not run exactly",
    );
    assert_flags(
        &m(&|s| s.replace(&step, &format!("        run: {CMD} --root /tmp\n"))),
        "passes --root",
    );
    assert_flags(
        &m(&|s| s.replace(&step, &format!("        run: {CMD} || true\n"))),
        "swallows failures",
    );
    assert_flags(
        &m(&|s| {
            s.replace(
                &format!("      - name: {CMD}\n"),
                &format!("      - name: {CMD}\n        if: false\n"),
            )
        }),
        "has a conditional",
    );
    assert_flags(
        &m(&|s| {
            s.replace(
                &format!("      - name: {CMD}\n"),
                &format!("      - name: {CMD}\n        continue-on-error: true\n"),
            )
        }),
        "continue-on-error",
    );
    assert_flags(
        &m(&|s| s.replace("needs: [fmt, clippy,", "needs: [fmt,")),
        "`ci-ok.needs` does not include `clippy`",
    );
}
