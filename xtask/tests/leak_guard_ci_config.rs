//! `ci.yml` wiring for story US-RR-13, task US-RR-13-6: the ROM-leak guard
//! runs on every push and pull request, as part of the one required check.
//!
//! [`leak_guard_problems`] is a pure check over the text of
//! `.github/workflows/ci.yml`; one test runs it on the committed file and the
//! `*_rejects_*` test proves each assertion bites on a mutated copy. It
//! asserts that job `leak-guard`:
//!
//! * exists, runs on Linux, has a timeout, and is in `ci-ok.needs`; the
//!   workflow runs on `push` and `pull_request`;
//! * runs exactly `cargo xtask leak-guard` (no paths: all of
//!   `fixtures/public`, every rule) and runs the guard over every tracked
//!   file, NUL-separated (`git ls-files -z | xargs -0 cargo xtask leak-guard
//!   --`), so a stock-ROM hash anywhere in the repo fails;
//! * passes no option that changes what the guard knows (`--manifest`,
//!   `--coverage`, `--root`): the committed data, on the checked-out tree;
//! * cannot be skipped or softened (`if:`, `continue-on-error`, `|| true`).
//!
//! What the guard rejects is proved by `xtask/tests/leak_guard.rs`, and the
//! pre-commit hook by `xtask/tests/leak_guard_hook.rs`, which also runs this
//! job's scripts, as committed, over a checkout holding a stock-ROM-shaped
//! image committed past the hook (US-RR-13-1); whether the job is green on
//! GitHub can only be seen there.

#[path = "common/workflow.rs"]
mod workflow;

use workflow::{
    Workflow, assert_flags, ci_yml, gate_needs_problems, skippable_problems, trigger_problems,
};

const JOB: &str = "leak-guard";
const PUBLIC: &str = "cargo xtask leak-guard";
const TRACKED: &str = "git ls-files -z | xargs -0 cargo xtask leak-guard --";

/// Every way `src` (ci.yml) fails the US-RR-13-6 wiring; empty when it holds.
fn leak_guard_problems(src: &str) -> Vec<String> {
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
        problems.push(format!("`{JOB}` has no timeout-minutes"));
    }
    let scripts = job.run_scripts();
    for want in [PUBLIC, TRACKED] {
        if !scripts.iter().any(|s| s == want) {
            problems.push(format!(
                "`{JOB}` does not run exactly `{want}` (scripts: {scripts:?})"
            ));
        }
    }
    for s in scripts.iter().filter(|s| s.contains("leak-guard")) {
        for opt in ["--manifest", "--coverage", "--root"] {
            if s.contains(opt) {
                problems.push(format!("`{JOB}` passes {opt}: {s}"));
            }
        }
    }
    problems.extend(skippable_problems(job, &[]));
    problems
}

#[test]
fn ci_runs_the_leak_guard_as_a_gate() {
    let problems = leak_guard_problems(&ci_yml());
    assert!(
        problems.is_empty(),
        ".github/workflows/ci.yml violates US-RR-13-6:\n{}",
        problems.join("\n")
    );
}

#[test]
fn ci_leak_guard_check_rejects_weakened_job() {
    let m = |edit: &dyn Fn(&str) -> String| workflow::mutated(leak_guard_problems, edit);
    let public = format!("        run: {PUBLIC}\n");
    let tracked = format!("        run: {TRACKED}\n");

    assert_flags(
        &m(&|s| s.replace(", leak-guard, ", ", ")),
        "`ci-ok.needs` does not include `leak-guard`",
    );
    assert_flags(
        &m(&|s| s.replace("\n  leak-guard:\n", "\n  rom-guard:\n")),
        "no `leak-guard` job",
    );
    assert_flags(
        &m(&|s| {
            s.replace(
                &public,
                "        run: cargo xtask leak-guard fixtures/public/synthetic\n",
            )
        }),
        "does not run exactly `cargo xtask leak-guard`",
    );
    assert_flags(
        &m(&|s| {
            s.replace(
                &tracked,
                "        run: git ls-files | xargs cargo xtask leak-guard --\n",
            )
        }),
        "does not run exactly `git ls-files -z",
    );
    assert_flags(
        &m(&|s| {
            s.replace(
                &public,
                &format!("{public}      - run: cargo xtask leak-guard --root /tmp\n"),
            )
        }),
        "passes --root",
    );
    assert_flags(
        &m(&|s| s.replace(&tracked, &format!("        run: {TRACKED} || true\n"))),
        "swallows failures",
    );
    assert_flags(
        &m(&|s| {
            s.replace(
                &tracked,
                &format!("        if: github.event_name == 'push'\n{tracked}"),
            )
        }),
        "has a conditional",
    );
    assert_flags(
        &m(&|s| {
            s.replace(
                "    name: ROM-leak guard\n    runs-on: ubuntu-24.04\n",
                "    name: ROM-leak guard\n    runs-on: windows-2022\n",
            )
        }),
        "does not run on an ubuntu- runner",
    );
}
